//! Conservative, render-graph-only precomposition helpers.
//!
//! The helpers operate on the existing render DAG and do not change project
//! timing, clips, or the session history. They accept only a single-output
//! selection whose boundary uses source/generator inputs and optional
//! `Transform2D` consumers. Matte, multi-input compositing, 3D scenes, and
//! post-processing nodes may be grouped inside a precomp, but may not be split
//! across its boundary.

use crate::error::SchemaError;
use crate::graph::{GraphNodeDef, NodeId, NodeKind, RenderGraphDef};
use std::collections::{BTreeMap, BTreeSet};

/// Moves a set of root-graph nodes into a `Precomp` and rewires its one output.
///
/// This operation is transactional: on failure, `graph` is unchanged. New
/// boundary inputs are represented by local `PrecompInput` nodes, preserving
/// the external dependency order from the original node/input traversal.
pub fn precompose(
    graph: &mut RenderGraphDef,
    selected_node_ids: &[NodeId],
    precomp_node_id: NodeId,
) -> Result<(), SchemaError> {
    let fail = |message: String| SchemaError::InvalidPrecompOperation { message };
    if selected_node_ids.is_empty() {
        return Err(fail("at least one node must be selected".into()));
    }
    graph.validate_graph()?;

    let selected: BTreeSet<NodeId> = selected_node_ids.iter().cloned().collect();
    if selected.len() != selected_node_ids.len() {
        return Err(fail("the selection contains duplicate node ids".into()));
    }
    for id in &selected {
        if graph.node(id).is_none() {
            return Err(SchemaError::MissingNode(id.clone()));
        }
    }
    if graph.find_node_recursive(&precomp_node_id).is_some() {
        return Err(fail(format!(
            "node id '{precomp_node_id}' is already in use"
        )));
    }

    let selected_nodes: Vec<GraphNodeDef> = graph
        .nodes
        .iter()
        .filter(|node| selected.contains(&node.id))
        .cloned()
        .collect();

    let mut exposed_outputs = BTreeSet::new();
    if selected.contains(&graph.output_node) {
        exposed_outputs.insert(graph.output_node.clone());
    }
    let mut external_inputs = Vec::<NodeId>::new();
    let mut input_slots = BTreeMap::<NodeId, usize>::new();

    for node in &selected_nodes {
        if matches!(node.kind, NodeKind::Scene3D { .. }) {
            return Err(fail(format!(
                "3D scene node '{}' cannot be precomposed in v1",
                node.id
            )));
        }
        for input in &node.inputs {
            if selected.contains(input) {
                continue;
            }
            let outside = graph
                .node(input)
                .expect("validated graph contains every input node");
            if !is_leaf_boundary_source(&outside.kind) {
                return Err(fail(format!(
                    "boundary input '{}' -> '{}' crosses a matte, composite, or post-processing dependency",
                    outside.id, node.id
                )));
            }
            if is_multi_input_boundary_kind(&node.kind) {
                return Err(fail(format!(
                    "boundary input into multi-input node '{}' would split a matte, blend, or transition dependency",
                    node.id
                )));
            }
            if !input_slots.contains_key(input) {
                let slot = external_inputs.len();
                external_inputs.push(input.clone());
                input_slots.insert(input.clone(), slot);
            }
        }
    }

    for outside in &graph.nodes {
        if selected.contains(&outside.id) {
            continue;
        }
        let depth_plate_id = if let NodeKind::Scene3D {
            depth_plate: Some(dp),
            ..
        } = &outside.kind
        {
            Some(&dp.node_id)
        } else {
            None
        };
        for input in outside.inputs.iter().chain(depth_plate_id) {
            if selected.contains(input) {
                if !matches!(outside.kind, NodeKind::Transform2D { .. }) {
                    return Err(fail(format!(
                        "boundary output '{}' -> '{}' crosses a matte, composite, 3D, or post-processing dependency",
                        input, outside.id
                    )));
                }
                exposed_outputs.insert(input.clone());
            }
        }
    }

    if exposed_outputs.len() != 1 {
        return Err(fail(format!(
            "selection must expose exactly one output node, found {}",
            exposed_outputs.len()
        )));
    }
    let selected_output = exposed_outputs
        .into_iter()
        .next()
        .expect("one output was checked above");

    let mut child_nodes = selected_nodes;
    let mut child_input_ids = Vec::with_capacity(external_inputs.len());
    let mut used_child_ids: BTreeSet<NodeId> = child_nodes.iter().map(|n| n.id.clone()).collect();
    for (slot, external_id) in external_inputs.iter().enumerate() {
        let input_id = fresh_precomp_input_id(&precomp_node_id, slot, &used_child_ids);
        used_child_ids.insert(input_id.clone());
        child_nodes.push(GraphNodeDef {
            id: input_id.clone(),
            kind: NodeKind::PrecompInput { slot },
            inputs: Vec::new(),
            enabled: true,
        });
        child_input_ids.push((external_id.clone(), input_id));
    }
    let input_id_by_external: BTreeMap<NodeId, NodeId> = child_input_ids.into_iter().collect();
    for node in &mut child_nodes {
        for input in &mut node.inputs {
            if !selected.contains(input) {
                *input = input_id_by_external
                    .get(input)
                    .expect("external input was assigned a slot")
                    .clone();
            }
        }
    }

    let mut candidate = graph.clone();
    candidate.nodes.retain(|node| !selected.contains(&node.id));
    let insertion_index = graph
        .nodes
        .iter()
        .position(|node| selected.contains(&node.id))
        .unwrap_or(candidate.nodes.len())
        .min(candidate.nodes.len());
    candidate.nodes.insert(
        insertion_index,
        GraphNodeDef {
            id: precomp_node_id.clone(),
            kind: NodeKind::Precomp {
                sub_graph: RenderGraphDef {
                    output_node: selected_output.clone(),
                    nodes: child_nodes,
                },
                time_offset_ticks: 0,
                time_scale: 1.0,
                opacity: 1.0,
            },
            inputs: external_inputs,
            enabled: true,
        },
    );

    for node in &mut candidate.nodes {
        if node.id == precomp_node_id {
            continue;
        }
        for input in &mut node.inputs {
            if selected.contains(input) {
                *input = precomp_node_id.clone();
            }
        }
        if let NodeKind::Scene3D {
            depth_plate: Some(dp),
            ..
        } = &mut node.kind
            && selected.contains(&dp.node_id)
        {
            dp.node_id = precomp_node_id.clone();
        }
    }
    if selected.contains(&candidate.output_node) {
        candidate.output_node = precomp_node_id;
    }

    candidate.validate_graph()?;
    *graph = candidate;
    Ok(())
}

/// Expands a root-level `Precomp` back into its parent graph.
///
/// The function reverses the structural rewrite made by [`precompose`]. It
/// also supports valid precomps authored elsewhere, provided every
/// `PrecompInput` slot maps to a parent input and node ids do not collide.
/// Like [`precompose`], it leaves the graph unchanged on error.
pub fn unprecompose(
    graph: &mut RenderGraphDef,
    precomp_node_id: &NodeId,
) -> Result<(), SchemaError> {
    let fail = |message: String| SchemaError::InvalidPrecompOperation { message };
    graph.validate_graph()?;
    let index = graph
        .nodes
        .iter()
        .position(|node| &node.id == precomp_node_id)
        .ok_or_else(|| SchemaError::MissingNode(precomp_node_id.clone()))?;
    let precomp_node = graph.nodes[index].clone();
    let NodeKind::Precomp {
        sub_graph,
        time_offset_ticks,
        time_scale,
        opacity,
    } = precomp_node.kind
    else {
        return Err(fail(format!("node '{precomp_node_id}' is not a precomp")));
    };
    if time_offset_ticks != 0 || time_scale != 1.0 || opacity != 1.0 || !precomp_node.enabled {
        return Err(fail(format!(
            "precomp '{precomp_node_id}' has non-default timing, opacity, or enabled state; opening it would change the rendered result"
        )));
    }

    let mut candidate = graph.clone();
    candidate.nodes.remove(index);
    let parent_ids: BTreeSet<NodeId> = candidate.nodes.iter().map(|node| node.id.clone()).collect();
    let mut input_replacements = BTreeMap::<NodeId, NodeId>::new();
    for node in &sub_graph.nodes {
        if let NodeKind::PrecompInput { slot } = node.kind {
            let parent_input = precomp_node.inputs.get(slot).ok_or_else(|| {
                fail(format!(
                    "precomp input node '{}' requests missing parent slot {slot}",
                    node.id
                ))
            })?;
            input_replacements.insert(node.id.clone(), parent_input.clone());
        }
    }

    let mut expanded_nodes = sub_graph.nodes.clone();
    for node in &mut expanded_nodes {
        for input in &mut node.inputs {
            if let Some(parent) = input_replacements.get(input) {
                *input = parent.clone();
            }
        }
        if let NodeKind::Scene3D {
            depth_plate: Some(dp),
            ..
        } = &mut node.kind
            && let Some(parent) = input_replacements.get(&dp.node_id)
        {
            dp.node_id = parent.clone();
        }
    }
    expanded_nodes.retain(|node| !matches!(node.kind, NodeKind::PrecompInput { .. }));

    let expanded_output = input_replacements
        .get(&sub_graph.output_node)
        .cloned()
        .unwrap_or_else(|| sub_graph.output_node.clone());
    if (parent_ids.contains(&expanded_output)
        || candidate.find_node_recursive(&expanded_output).is_some())
        && !input_replacements.values().any(|id| id == &expanded_output)
    {
        return Err(fail(format!(
            "expanded output id '{}' collides with a parent node",
            expanded_output
        )));
    }
    for node in &expanded_nodes {
        if parent_ids.contains(&node.id) || candidate.find_node_recursive(&node.id).is_some() {
            return Err(fail(format!(
                "expanded node id '{}' collides with a parent node",
                node.id
            )));
        }
    }

    for node in &mut candidate.nodes {
        for input in &mut node.inputs {
            if input == precomp_node_id {
                *input = expanded_output.clone();
            }
        }
        if let NodeKind::Scene3D {
            depth_plate: Some(dp),
            ..
        } = &mut node.kind
            && &dp.node_id == precomp_node_id
        {
            dp.node_id = expanded_output.clone();
        }
    }
    if candidate.output_node == *precomp_node_id {
        candidate.output_node = expanded_output;
    }
    candidate.nodes.splice(index..index, expanded_nodes);
    candidate.validate_graph()?;
    *graph = candidate;
    Ok(())
}

fn fresh_precomp_input_id(precomp_id: &NodeId, slot: usize, used: &BTreeSet<NodeId>) -> NodeId {
    let base = format!("{}__input_{slot}", precomp_id.0);
    let mut candidate = base.clone();
    let mut suffix = 1usize;
    while used.contains(&NodeId(candidate.clone())) {
        candidate = format!("{base}_{suffix}");
        suffix += 1;
    }
    NodeId(candidate)
}

fn is_leaf_boundary_source(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::SourceVideo { .. }
            | NodeKind::SourceImage { .. }
            | NodeKind::Solid { .. }
            | NodeKind::VectorText { .. }
            | NodeKind::Text3D { .. }
    )
}

fn is_multi_input_boundary_kind(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::MaskMatte { .. }
            | NodeKind::DepthMatte { .. }
            | NodeKind::Blend { .. }
            | NodeKind::ShaderTransition { .. }
            | NodeKind::Scene3D { .. }
    )
}
