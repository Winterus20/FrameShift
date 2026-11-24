use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default fuel budget for `SafeExpr::eval_default`.
pub const DEFAULT_MAX_FUEL: u32 = 256;

/// Maximum AST nesting depth allowed during parsing (prevents stack exhaustion).
pub const MAX_EXPR_DEPTH: usize = 64;

/// Maximum AST node count allowed during parsing (prevents unbounded expressions).
pub const MAX_EXPR_NODES: usize = 1024;

/// Errors produced when parsing or evaluating a `SafeExpr`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ExprError {
    #[error("expression syntax error: {0}")]
    SyntaxError(String),

    #[error("unknown identifier '{0}'")]
    UnknownIdentifier(String),

    #[error("unknown function '{0}'")]
    UnknownFunction(String),

    #[error("function '{func}' expects {expected} argument(s), found {actual}")]
    InvalidArity {
        func: String,
        expected: usize,
        actual: usize,
    },

    #[error("division by zero in expression")]
    DivisionByZero,

    #[error("non-finite floating-point value (NaN or Inf) in expression")]
    NonFiniteValue,

    #[error("invalid function argument: {0}")]
    InvalidArgument(String),

    #[error("expression evaluation exhausted fuel budget ({max_fuel})")]
    FuelExhausted { max_fuel: u32 },

    #[error("expression nesting exceeds maximum depth ({max_depth})")]
    MaxDepthExceeded { max_depth: usize },

    #[error("expression exceeds maximum AST node count ({max_nodes})")]
    MaxAstNodesExceeded { max_nodes: usize },
}

/// Evaluation context passed to `SafeExpr::eval`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ExprContext {
    pub t_sec: f64,
    pub output_tick: i64,
    pub beat_phase: f64,
    pub audio_rms: f64,
    pub audio_sub: f64,
    pub audio_mid: f64,
    pub audio_high: f64,
    pub downbeat_phase: f64,
    pub event_delta_sec: f64,
    pub progress: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextVar {
    TSec,
    OutputTick,
    BeatPhase,
    AudioRms,
    AudioSub,
    AudioMid,
    AudioHigh,
    DownbeatPhase,
    EventDeltaSec,
    Progress,
}

impl ContextVar {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "t_sec" => Some(Self::TSec),
            "output_tick" => Some(Self::OutputTick),
            "beat_phase" => Some(Self::BeatPhase),
            "audio_rms" => Some(Self::AudioRms),
            "audio_sub" => Some(Self::AudioSub),
            "audio_mid" => Some(Self::AudioMid),
            "audio_high" => Some(Self::AudioHigh),
            "downbeat_phase" => Some(Self::DownbeatPhase),
            "event_delta_sec" => Some(Self::EventDeltaSec),
            "progress" => Some(Self::Progress),
            _ => None,
        }
    }

    fn read(self, ctx: &ExprContext) -> Result<f64, ExprError> {
        let val = match self {
            Self::TSec => ctx.t_sec,
            Self::OutputTick => ctx.output_tick as f64,
            Self::BeatPhase => ctx.beat_phase,
            Self::AudioRms => ctx.audio_rms,
            Self::AudioSub => ctx.audio_sub,
            Self::AudioMid => ctx.audio_mid,
            Self::AudioHigh => ctx.audio_high,
            Self::DownbeatPhase => ctx.downbeat_phase,
            Self::EventDeltaSec => ctx.event_delta_sec,
            Self::Progress => ctx.progress,
        };
        ensure_finite(val)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnaryOp {
    Neg,
    Pos,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogicalOp {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuiltinFunc {
    Sin,
    Cos,
    Abs,
    Exp,
    Floor,
    Ceil,
    Fract,
    Sqrt,
    Log,
    EaseOutExpo,
    EaseInExpo,
    EaseInOutExpo,
    EaseOutBack,
    Min,
    Max,
    Mod,
    Clamp,
    Smoothstep,
    Lerp,
    Mix,
    Step,
    Pow,
    Spring,
    Wiggle,
    CubicBezier,
}

impl BuiltinFunc {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "sin" => Some(Self::Sin),
            "cos" => Some(Self::Cos),
            "abs" => Some(Self::Abs),
            "exp" => Some(Self::Exp),
            "floor" => Some(Self::Floor),
            "ceil" => Some(Self::Ceil),
            "fract" => Some(Self::Fract),
            "sqrt" => Some(Self::Sqrt),
            "log" => Some(Self::Log),
            "ease_out_expo" => Some(Self::EaseOutExpo),
            "ease_in_expo" => Some(Self::EaseInExpo),
            "ease_in_out_expo" => Some(Self::EaseInOutExpo),
            "ease_out_back" => Some(Self::EaseOutBack),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            "mod" => Some(Self::Mod),
            "clamp" => Some(Self::Clamp),
            "smoothstep" => Some(Self::Smoothstep),
            "lerp" => Some(Self::Lerp),
            "mix" => Some(Self::Mix),
            "step" => Some(Self::Step),
            "pow" => Some(Self::Pow),
            "spring" => Some(Self::Spring),
            "wiggle" => Some(Self::Wiggle),
            "cubic_bezier" => Some(Self::CubicBezier),
            _ => None,
        }
    }

    fn arity(self) -> usize {
        match self {
            Self::Sin
            | Self::Cos
            | Self::Abs
            | Self::Exp
            | Self::Floor
            | Self::Ceil
            | Self::Fract
            | Self::Sqrt
            | Self::Log
            | Self::EaseOutExpo
            | Self::EaseInExpo
            | Self::EaseInOutExpo
            | Self::EaseOutBack => 1,
            Self::Min | Self::Max | Self::Mod | Self::Step | Self::Pow => 2,
            Self::Clamp
            | Self::Smoothstep
            | Self::Lerp
            | Self::Mix
            | Self::Spring
            | Self::Wiggle => 3,
            Self::CubicBezier => 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ExprNode {
    Literal(f64),
    Var(ContextVar),
    Unary {
        op: UnaryOp,
        expr: Box<ExprNode>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<ExprNode>,
        rhs: Box<ExprNode>,
    },
    Logical {
        op: LogicalOp,
        lhs: Box<ExprNode>,
        rhs: Box<ExprNode>,
    },
    Cond {
        cond: Box<ExprNode>,
        then_branch: Box<ExprNode>,
        else_branch: Box<ExprNode>,
    },
    Call {
        func: BuiltinFunc,
        args: Vec<ExprNode>,
    },
}

/// Acyclic, fuel-limited arithmetic expression compiled into an immutable AST.
#[derive(Debug, Clone, PartialEq)]
pub struct SafeExpr {
    source: String,
    root: ExprNode,
    node_count: usize,
}

impl SafeExpr {
    /// Parses `source` into an acyclic AST with default structural bounds.
    pub fn parse(source: &str) -> Result<Self, ExprError> {
        let tokens = tokenize(source)?;
        if tokens.is_empty() {
            return Err(ExprError::SyntaxError("empty expression".into()));
        }
        let mut parser = Parser {
            tokens: &tokens,
            pos: 0,
            node_count: 0,
        };
        let root = parser.parse_expr(0)?;
        if parser.pos < parser.tokens.len() {
            return Err(ExprError::SyntaxError(format!(
                "unexpected token {:?}",
                parser.tokens[parser.pos]
            )));
        }
        Ok(Self {
            source: source.to_owned(),
            root,
            node_count: parser.node_count,
        })
    }

    /// Original expression string.
    #[inline]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Number of AST nodes in this compiled expression.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// Evaluates the expression against `ctx` with a strict `max_fuel` step budget.
    pub fn eval(&self, ctx: &ExprContext, max_fuel: u32) -> Result<f64, ExprError> {
        let mut remaining_fuel = max_fuel;
        let val = eval_node(&self.root, ctx, &mut remaining_fuel, max_fuel)?;
        ensure_finite(val)
    }

    /// Evaluates the expression against `ctx` with [`DEFAULT_MAX_FUEL`].
    pub fn eval_default(&self, ctx: &ExprContext) -> Result<f64, ExprError> {
        self.eval(ctx, DEFAULT_MAX_FUEL)
    }
}

#[inline]
fn eval_unit_cubic_bezier(t: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let t_clamped = t.clamp(0.0, 1.0);
    if t_clamped <= 0.0 {
        return 0.0;
    }
    if t_clamped >= 1.0 {
        return 1.0;
    }
    let u = crate::speed_curve::solve_cubic_bezier_x(x1, x2, t_clamped);
    crate::speed_curve::eval_cubic_bezier_1d(0.0, y1, y2, 1.0, u)
}

#[inline]
fn ensure_finite(val: f64) -> Result<f64, ExprError> {
    if val.is_finite() {
        Ok(val)
    } else {
        Err(ExprError::NonFiniteValue)
    }
}

/// Analytical step-response of a damped harmonic oscillator (seek-independent).
///
/// `x(0) = 0`, `x'(0) = 0`, `lim_{t -> inf} x(t) = 1.0`.
#[inline]
fn eval_spring(t: f64, stiffness: f64, damping: f64) -> f64 {
    if t <= 0.0 {
        return 0.0;
    }
    let omega0 = stiffness.clamp(0.01, 10_000.0).sqrt();
    let zeta = damping.clamp(0.0, 10.0);
    if zeta > 0.0 && t >= 30.0 && zeta * omega0 * t > 40.0 {
        return 1.0;
    }
    let env = (-zeta * omega0 * t).exp();
    if zeta < 1.0 - 1e-5 {
        let denom = (1.0 - zeta * zeta).sqrt();
        let wd = omega0 * denom;
        let c = zeta / denom;
        1.0 - env * ((wd * t).cos() + c * (wd * t).sin())
    } else if zeta <= 1.0 + 1e-5 {
        1.0 - env * (1.0 + omega0 * t)
    } else {
        let r = (zeta * zeta - 1.0).sqrt();
        let r1 = -omega0 * (zeta - r);
        let r2 = -omega0 * (zeta + r);
        let denom = r2 - r1;
        let c1 = r2 / denom;
        let c2 = -r1 / denom;
        1.0 - (c1 * (r1 * t).exp() + c2 * (r2 * t).exp())
    }
}

#[inline]
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[inline]
fn grad_1d(cell: i64, seed: u64) -> f64 {
    let h = splitmix64((cell as u64).wrapping_mul(0xd6e8_feb8_6659_fd93) ^ seed);
    let unit = ((h >> 11) as f64) / ((1u64 << 53) as f64);
    unit * 2.0 - 1.0
}

/// Deterministic, seek-independent 1D gradient (Perlin) noise scaled to `[-amp, +amp]`.
#[inline]
fn eval_wiggle(t_sec: f64, freq: f64, amp: f64, seed: f64) -> f64 {
    if !freq.is_finite() || !amp.is_finite() || !seed.is_finite() || freq <= 0.0 || amp == 0.0 {
        return 0.0;
    }
    let u = t_sec * freq;
    let fl = u.floor();
    let i0 = fl as i64;
    let f = u - fl;
    let s = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let seed_u64 = (seed.round() as i64) as u64;
    let n0 = grad_1d(i0, seed_u64) * f;
    let n1 = grad_1d(i0.wrapping_add(1), seed_u64) * (f - 1.0);
    (n0 + (n1 - n0) * s) * 2.0 * amp
}

#[inline]
fn consume_extra_fuel(fuel: &mut u32, extra: u32, max_fuel: u32) -> Result<(), ExprError> {
    if *fuel < extra {
        return Err(ExprError::FuelExhausted { max_fuel });
    }
    *fuel -= extra;
    Ok(())
}

fn eval_node(
    node: &ExprNode,
    ctx: &ExprContext,
    fuel: &mut u32,
    max_fuel: u32,
) -> Result<f64, ExprError> {
    if *fuel == 0 {
        return Err(ExprError::FuelExhausted { max_fuel });
    }
    *fuel -= 1;

    match node {
        ExprNode::Literal(v) => ensure_finite(*v),
        ExprNode::Var(var) => var.read(ctx),
        ExprNode::Unary { op, expr } => {
            let v = eval_node(expr, ctx, fuel, max_fuel)?;
            let out = match op {
                UnaryOp::Neg => -v,
                UnaryOp::Pos => v,
                UnaryOp::Not => {
                    if v == 0.0 {
                        1.0
                    } else {
                        0.0
                    }
                }
            };
            ensure_finite(out)
        }
        ExprNode::Binary { op, lhs, rhs } => {
            let a = eval_node(lhs, ctx, fuel, max_fuel)?;
            let b = eval_node(rhs, ctx, fuel, max_fuel)?;
            let out = match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a - b,
                BinaryOp::Mul => a * b,
                BinaryOp::Div => {
                    if b == 0.0 {
                        return Err(ExprError::DivisionByZero);
                    }
                    a / b
                }
                BinaryOp::Mod => {
                    if b == 0.0 {
                        return Err(ExprError::DivisionByZero);
                    }
                    let q = ensure_finite(a / b)?;
                    a - b * q.floor()
                }
                BinaryOp::Lt => if a < b { 1.0 } else { 0.0 },
                BinaryOp::Le => if a <= b { 1.0 } else { 0.0 },
                BinaryOp::Gt => if a > b { 1.0 } else { 0.0 },
                BinaryOp::Ge => if a >= b { 1.0 } else { 0.0 },
                BinaryOp::Eq => if a == b { 1.0 } else { 0.0 },
                BinaryOp::Ne => if a != b { 1.0 } else { 0.0 },
            };
            ensure_finite(out)
        }
        ExprNode::Logical { op, lhs, rhs } => {
            let a = eval_node(lhs, ctx, fuel, max_fuel)?;
            match op {
                LogicalOp::And => {
                    if a == 0.0 {
                        return Ok(0.0);
                    }
                    let b = eval_node(rhs, ctx, fuel, max_fuel)?;
                    Ok(if b != 0.0 { 1.0 } else { 0.0 })
                }
                LogicalOp::Or => {
                    if a != 0.0 {
                        return Ok(1.0);
                    }
                    let b = eval_node(rhs, ctx, fuel, max_fuel)?;
                    Ok(if b != 0.0 { 1.0 } else { 0.0 })
                }
            }
        }
        ExprNode::Cond {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = eval_node(cond, ctx, fuel, max_fuel)?;
            if c != 0.0 {
                eval_node(then_branch, ctx, fuel, max_fuel)
            } else {
                eval_node(else_branch, ctx, fuel, max_fuel)
            }
        }
        ExprNode::Call { func, args } => {
            let mut vals = [0.0_f64; 5];
            for (i, arg) in args.iter().enumerate() {
                vals[i] = eval_node(arg, ctx, fuel, max_fuel)?;
            }
            let out = match func {
                BuiltinFunc::Sin => vals[0].sin(),
                BuiltinFunc::Cos => vals[0].cos(),
                BuiltinFunc::Abs => vals[0].abs(),
                BuiltinFunc::Exp => vals[0].exp(),
                BuiltinFunc::Floor => vals[0].floor(),
                BuiltinFunc::Ceil => vals[0].ceil(),
                BuiltinFunc::Fract => vals[0] - vals[0].floor(),
                BuiltinFunc::Sqrt => vals[0].max(0.0).sqrt(),
                BuiltinFunc::Log => vals[0].max(1e-12).ln(),
                BuiltinFunc::EaseOutExpo => {
                    let t = vals[0].clamp(0.0, 1.0);
                    if t <= 0.0 {
                        0.0
                    } else if t >= 1.0 {
                        1.0
                    } else {
                        1.0 - 2.0_f64.powf(-10.0 * t)
                    }
                }
                BuiltinFunc::EaseInExpo => {
                    let t = vals[0].clamp(0.0, 1.0);
                    if t <= 0.0 {
                        0.0
                    } else if t >= 1.0 {
                        1.0
                    } else {
                        2.0_f64.powf(10.0 * t - 10.0)
                    }
                }
                BuiltinFunc::EaseInOutExpo => {
                    let t = vals[0].clamp(0.0, 1.0);
                    if t <= 0.0 {
                        0.0
                    } else if t >= 1.0 {
                        1.0
                    } else if t < 0.5 {
                        0.5 * 2.0_f64.powf(20.0 * t - 10.0)
                    } else {
                        1.0 - 0.5 * 2.0_f64.powf(-20.0 * t + 10.0)
                    }
                }
                BuiltinFunc::EaseOutBack => {
                    const C1: f64 = 1.70158;
                    const C3: f64 = C1 + 1.0;
                    let t = vals[0].clamp(0.0, 1.0);
                    if t <= 0.0 {
                        0.0
                    } else if t >= 1.0 {
                        1.0
                    } else {
                        let u = t - 1.0;
                        1.0 + C3 * u * u * u + C1 * u * u
                    }
                }
                BuiltinFunc::Min => vals[0].min(vals[1]),
                BuiltinFunc::Max => vals[0].max(vals[1]),
                BuiltinFunc::Mod => {
                    let (a, b) = (vals[0], vals[1]);
                    if b == 0.0 {
                        return Err(ExprError::DivisionByZero);
                    }
                    let q = ensure_finite(a / b)?;
                    a - b * q.floor()
                }
                BuiltinFunc::Clamp => {
                    let (x, lo, hi) = (vals[0], vals[1], vals[2]);
                    if lo > hi {
                        return Err(ExprError::InvalidArgument(format!(
                            "clamp requires min <= max, got [{lo}, {hi}]"
                        )));
                    }
                    x.clamp(lo, hi)
                }
                BuiltinFunc::Smoothstep => {
                    let (edge0, edge1, x) = (vals[0], vals[1], vals[2]);
                    if edge0 == edge1 {
                        return Err(ExprError::DivisionByZero);
                    }
                    if edge0 > edge1 {
                        return Err(ExprError::InvalidArgument(format!(
                            "smoothstep requires edge0 < edge1, got [{edge0}, {edge1}]"
                        )));
                    }
                    let u = (x - edge0) / (edge1 - edge0);
                    if !u.is_finite() {
                        return Err(ExprError::NonFiniteValue);
                    }
                    let t = u.clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                }
                BuiltinFunc::Lerp | BuiltinFunc::Mix => {
                    let (a, b, t) = (vals[0], vals[1], vals[2]);
                    a + (b - a) * t
                }
                BuiltinFunc::Step => {
                    let (edge, x) = (vals[0], vals[1]);
                    if x < edge { 0.0 } else { 1.0 }
                }
                BuiltinFunc::Pow => {
                    let (base, exp) = (vals[0], vals[1]);
                    if base < 0.0 && exp.fract() != 0.0 {
                        0.0
                    } else {
                        base.powf(exp)
                    }
                }
                BuiltinFunc::Spring => {
                    consume_extra_fuel(fuel, 3, max_fuel)?;
                    eval_spring(vals[0], vals[1], vals[2])
                }
                BuiltinFunc::Wiggle => {
                    consume_extra_fuel(fuel, 3, max_fuel)?;
                    let t_sec = ensure_finite(ctx.t_sec)?;
                    eval_wiggle(t_sec, vals[0], vals[1], vals[2])
                }
                BuiltinFunc::CubicBezier => {
                    let (t_in, x1, y1, x2, y2) = (vals[0], vals[1], vals[2], vals[3], vals[4]);
                    if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
                        return Err(ExprError::InvalidArgument(format!(
                            "cubic_bezier requires x1 and x2 in [0, 1], got [{x1}, {x2}]"
                        )));
                    }
                    eval_unit_cubic_bezier(t_in, x1, y1, x2, y2)
                }
            };
            ensure_finite(out)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    NotEq,
    AndAnd,
    OrOr,
    Bang,
    LParen,
    RParen,
    Comma,
}

fn tokenize(input: &str) -> Result<Vec<Token>, ExprError> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        match b {
            b'+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            b'-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            b'*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            b'/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            b'%' => {
                tokens.push(Token::Percent);
                i += 1;
            }
            b'<' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    tokens.push(Token::Le);
                    i += 2;
                } else {
                    tokens.push(Token::Lt);
                    i += 1;
                }
            }
            b'>' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    tokens.push(Token::Ge);
                    i += 2;
                } else {
                    tokens.push(Token::Gt);
                    i += 1;
                }
            }
            b'=' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    tokens.push(Token::EqEq);
                    i += 2;
                } else {
                    return Err(ExprError::SyntaxError(
                        "unexpected '='; use '==' for equality".into(),
                    ));
                }
            }
            b'!' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    tokens.push(Token::NotEq);
                    i += 2;
                } else {
                    tokens.push(Token::Bang);
                    i += 1;
                }
            }
            b'&' => {
                if bytes.get(i + 1) == Some(&b'&') {
                    tokens.push(Token::AndAnd);
                    i += 2;
                } else {
                    return Err(ExprError::SyntaxError(
                        "unexpected '&'; use '&&' for logical AND".into(),
                    ));
                }
            }
            b'|' => {
                if bytes.get(i + 1) == Some(&b'|') {
                    tokens.push(Token::OrOr);
                    i += 2;
                } else {
                    return Err(ExprError::SyntaxError(
                        "unexpected '|'; use '||' for logical OR".into(),
                    ));
                }
            }
            b'(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            b')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            b',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            b'0'..=b'9' | b'.' => {
                let start = i;
                let mut seen_dot = false;
                let mut seen_digit = false;

                while i < bytes.len() {
                    match bytes[i] {
                        b'0'..=b'9' => {
                            seen_digit = true;
                            i += 1;
                        }
                        b'.' if !seen_dot => {
                            seen_dot = true;
                            i += 1;
                        }
                        _ => break,
                    }
                }

                if !seen_digit {
                    return Err(ExprError::SyntaxError(
                        "unexpected '.' without digits".into(),
                    ));
                }

                if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                    i += 1;
                    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                        i += 1;
                    }
                    let exp_start = i;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i == exp_start {
                        return Err(ExprError::SyntaxError(
                            "malformed scientific notation exponent".into(),
                        ));
                    }
                }

                // Reject immediate trailing '.' or identifier characters attached to a number literal
                if i < bytes.len()
                    && (bytes[i] == b'.' || bytes[i].is_ascii_alphabetic() || bytes[i] == b'_')
                {
                    return Err(ExprError::SyntaxError(format!(
                        "invalid numeric literal starting at '{}'",
                        &input[start..=i]
                    )));
                }

                let slice = &input[start..i];
                let num: f64 = slice.parse().map_err(|_| {
                    ExprError::SyntaxError(format!("invalid numeric literal '{slice}'"))
                })?;
                if !num.is_finite() {
                    return Err(ExprError::NonFiniteValue);
                }
                tokens.push(Token::Number(num));
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                tokens.push(Token::Ident(input[start..i].to_owned()));
            }
            _ => {
                let ch = input[i..].chars().next().unwrap_or('?');
                return Err(ExprError::SyntaxError(format!(
                    "unexpected character '{ch}'"
                )));
            }
        }
    }

    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    node_count: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn alloc_node(&mut self, node: ExprNode) -> Result<ExprNode, ExprError> {
        self.node_count += 1;
        if self.node_count > MAX_EXPR_NODES {
            return Err(ExprError::MaxAstNodesExceeded {
                max_nodes: MAX_EXPR_NODES,
            });
        }
        Ok(node)
    }

    fn check_depth(depth: usize) -> Result<(), ExprError> {
        if depth > MAX_EXPR_DEPTH {
            return Err(ExprError::MaxDepthExceeded {
                max_depth: MAX_EXPR_DEPTH,
            });
        }
        Ok(())
    }

    fn parse_expr(&mut self, depth: usize) -> Result<ExprNode, ExprError> {
        self.parse_or(depth)
    }

    fn parse_or(&mut self, depth: usize) -> Result<ExprNode, ExprError> {
        Self::check_depth(depth)?;
        let mut lhs = self.parse_and(depth + 1)?;
        while matches!(self.peek(), Some(Token::OrOr)) {
            self.pos += 1;
            let rhs = self.parse_and(depth + 1)?;
            lhs = self.alloc_node(ExprNode::Logical {
                op: LogicalOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })?;
        }
        Ok(lhs)
    }

    fn parse_and(&mut self, depth: usize) -> Result<ExprNode, ExprError> {
        Self::check_depth(depth)?;
        let mut lhs = self.parse_eq(depth + 1)?;
        while matches!(self.peek(), Some(Token::AndAnd)) {
            self.pos += 1;
            let rhs = self.parse_eq(depth + 1)?;
            lhs = self.alloc_node(ExprNode::Logical {
                op: LogicalOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })?;
        }
        Ok(lhs)
    }

    fn parse_eq(&mut self, depth: usize) -> Result<ExprNode, ExprError> {
        Self::check_depth(depth)?;
        let mut lhs = self.parse_cmp(depth + 1)?;
        while let Some(tok) = self.peek() {
            let op = match tok {
                Token::EqEq => BinaryOp::Eq,
                Token::NotEq => BinaryOp::Ne,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_cmp(depth + 1)?;
            lhs = self.alloc_node(ExprNode::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })?;
        }
        Ok(lhs)
    }

    fn parse_cmp(&mut self, depth: usize) -> Result<ExprNode, ExprError> {
        Self::check_depth(depth)?;
        let mut lhs = self.parse_add(depth + 1)?;
        while let Some(tok) = self.peek() {
            let op = match tok {
                Token::Lt => BinaryOp::Lt,
                Token::Le => BinaryOp::Le,
                Token::Gt => BinaryOp::Gt,
                Token::Ge => BinaryOp::Ge,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_add(depth + 1)?;
            lhs = self.alloc_node(ExprNode::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })?;
        }
        Ok(lhs)
    }

    fn parse_add(&mut self, depth: usize) -> Result<ExprNode, ExprError> {
        Self::check_depth(depth)?;
        let mut lhs = self.parse_term(depth + 1)?;
        while let Some(tok) = self.peek() {
            let op = match tok {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_term(depth + 1)?;
            lhs = self.alloc_node(ExprNode::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })?;
        }
        Ok(lhs)
    }

    fn parse_term(&mut self, depth: usize) -> Result<ExprNode, ExprError> {
        Self::check_depth(depth)?;
        let mut lhs = self.parse_unary(depth + 1)?;
        while let Some(tok) = self.peek() {
            let op = match tok {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_unary(depth + 1)?;
            lhs = self.alloc_node(ExprNode::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })?;
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self, depth: usize) -> Result<ExprNode, ExprError> {
        Self::check_depth(depth)?;
        match self.peek() {
            Some(Token::Minus) => {
                self.pos += 1;
                let inner = self.parse_unary(depth + 1)?;
                self.alloc_node(ExprNode::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(inner),
                })
            }
            Some(Token::Plus) => {
                self.pos += 1;
                let inner = self.parse_unary(depth + 1)?;
                self.alloc_node(ExprNode::Unary {
                    op: UnaryOp::Pos,
                    expr: Box::new(inner),
                })
            }
            Some(Token::Bang) => {
                self.pos += 1;
                let inner = self.parse_unary(depth + 1)?;
                self.alloc_node(ExprNode::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(inner),
                })
            }
            _ => self.parse_primary(depth + 1),
        }
    }

    fn parse_primary(&mut self, depth: usize) -> Result<ExprNode, ExprError> {
        Self::check_depth(depth)?;
        let tok = self
            .peek()
            .cloned()
            .ok_or_else(|| ExprError::SyntaxError("unexpected end of expression".into()))?;

        match tok {
            Token::Number(n) => {
                self.pos += 1;
                self.alloc_node(ExprNode::Literal(n))
            }
            Token::Ident(name) => {
                self.pos += 1;
                if matches!(self.peek(), Some(Token::LParen)) {
                    self.pos += 1; // consume '('
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Token::RParen)) {
                        loop {
                            args.push(self.parse_expr(depth + 1)?);
                            match self.peek() {
                                Some(Token::Comma) => {
                                    self.pos += 1;
                                }
                                Some(Token::RParen) => break,
                                other => {
                                    return Err(ExprError::SyntaxError(format!(
                                        "expected ',' or ')' in call to '{name}', found {other:?}"
                                    )));
                                }
                            }
                        }
                    }
                    self.pos += 1; // consume ')'

                    if name == "if" || name == "select" {
                        if args.len() != 3 {
                            return Err(ExprError::InvalidArity {
                                func: name,
                                expected: 3,
                                actual: args.len(),
                            });
                        }
                        let mut iter = args.into_iter();
                        let cond = iter.next().unwrap();
                        let then_branch = iter.next().unwrap();
                        let else_branch = iter.next().unwrap();
                        return self.alloc_node(ExprNode::Cond {
                            cond: Box::new(cond),
                            then_branch: Box::new(then_branch),
                            else_branch: Box::new(else_branch),
                        });
                    }

                    let func = BuiltinFunc::from_name(&name)
                        .ok_or_else(|| ExprError::UnknownFunction(name.clone()))?;
                    let expected = func.arity();
                    if args.len() != expected {
                        return Err(ExprError::InvalidArity {
                            func: name,
                            expected,
                            actual: args.len(),
                        });
                    }
                    self.alloc_node(ExprNode::Call { func, args })
                } else {
                    let var = ContextVar::from_name(&name)
                        .ok_or(ExprError::UnknownIdentifier(name))?;
                    self.alloc_node(ExprNode::Var(var))
                }
            }
            Token::LParen => {
                self.pos += 1;
                let inner = self.parse_expr(depth + 1)?;
                match self.peek() {
                    Some(Token::RParen) => {
                        self.pos += 1;
                        Ok(inner)
                    }
                    other => Err(ExprError::SyntaxError(format!(
                        "expected ')', found {other:?}"
                    ))),
                }
            }
            other => Err(ExprError::SyntaxError(format!(
                "unexpected token {other:?}"
            ))),
        }
    }
}
