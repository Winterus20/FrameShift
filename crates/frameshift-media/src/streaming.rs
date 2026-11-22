use crate::error::MediaError;
use crate::ffmpeg::{FfmpegBridge, SourceFit, append_fitted_output, midpoint_seek_seconds};
use crate::frame::{AlphaMode, Rgba8Frame};
use frameshift_time::PtsIndex;
use std::collections::{BTreeMap, HashMap};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;

/// Default maximum number of concurrently open `ffmpeg` streaming subprocesses.
pub const DEFAULT_MAX_OPEN_STREAMS: usize = 8;

/// Maximum forward frame gap served by reading & discarding from an open pipe
/// instead of spawning a new `-ss` seek subprocess.
pub const DEFAULT_MAX_SEQUENTIAL_SKIP: usize = 32;

/// `BufReader` capacity (2 MiB) to reduce pipe `read` syscall overhead.
const STREAM_READ_BUF_BYTES: usize = 2 * 1024 * 1024;

/// Maximum stderr bytes retained per streaming subprocess to prevent unbounded growth.
const MAX_STDERR_CAPTURE_BYTES: usize = 64 * 1024;

/// Uniquely identifies an open FFmpeg rawvideo stream in [`DecoderPool`].
///
/// Separate slots are maintained for different output dimensions or [`SourceFit`] modes
/// even when reading from the same underlying media file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamKey {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fit: SourceFit,
}

impl StreamKey {
    /// Creates a new stream key for `(path, width, height, fit)`.
    pub fn new(path: impl Into<PathBuf>, width: u32, height: u32, fit: SourceFit) -> Self {
        Self {
            path: path.into(),
            width,
            height,
            fit,
        }
    }
}

/// Configuration for [`DecoderPool`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoderPoolConfig {
    pub max_open_streams: usize,
    pub max_sequential_skip: usize,
}

impl Default for DecoderPoolConfig {
    fn default() -> Self {
        Self {
            max_open_streams: DEFAULT_MAX_OPEN_STREAMS,
            max_sequential_skip: DEFAULT_MAX_SEQUENTIAL_SKIP,
        }
    }
}

/// Telemetry counters reported by [`DecoderPool`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DecoderPoolTelemetry {
    pub active_streams: usize,
    pub spawns: u64,
    pub sequential_reads: u64,
    pub skipped_frames: u64,
    pub seek_reopens: u64,
    pub lru_evictions: u64,
    pub eof_reaches: u64,
}

/// A single long-lived `ffmpeg` rawvideo RGBA stdout pipe session.
#[derive(Debug)]
pub struct StreamingVideoDecoder {
    key: StreamKey,
    child: Child,
    reader: Option<BufReader<ChildStdout>>,
    stderr_drain: Option<JoinHandle<String>>,
    /// Next `PtsIndex` frame index expected from `reader`.
    next_cursor: usize,
    frame_byte_len: usize,
    scratch_skip_buf: Vec<u8>,
    finished: bool,
    waited: bool,
}

impl StreamingVideoDecoder {
    /// Spawns a continuous rawvideo RGBA pipe starting at `start_frame_index`.
    ///
    /// Uses software decoding and identical scaling/seek flags to
    /// `FfmpegBridge::decode_video_window_with_fit`, without `-frames:v` and with
    /// `.stdin(Stdio::null())` to avoid inheriting JSON-RPC stdio.
    pub fn open(
        key: StreamKey,
        pts_index: &PtsIndex,
        start_frame_index: usize,
    ) -> Result<Self, MediaError> {
        if key.width == 0 || key.height == 0 {
            return Err(MediaError::InvalidDimensions {
                width: key.width,
                height: key.height,
            });
        }
        if pts_index.is_empty() || start_frame_index >= pts_index.len() {
            return Err(MediaError::EmptyMediaStream(format!(
                "cannot open video stream at frame {start_frame_index} (len={}) for '{}'",
                pts_index.len(),
                key.path.display()
            )));
        }
        let frame_byte_len = (key.width as usize)
            .checked_mul(key.height as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or(MediaError::InvalidDimensions {
                width: key.width,
                height: key.height,
            })?;

        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin"]);
        if let Some(seek_sec) = midpoint_seek_seconds(pts_index, start_frame_index) {
            cmd.arg("-ss").arg(format!("{seek_sec:.9}"));
        }
        cmd.arg("-i")
            .arg(&key.path)
            .args(["-fps_mode", "passthrough"]);
        append_fitted_output(&mut cmd, key.width, key.height, key.fit);
        cmd.args(["-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| MediaError::BinarySpawnFailed {
            binary: "ffmpeg".into(),
            reason: e.to_string(),
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            let _ = child.kill();
            let _ = child.wait();
            MediaError::FfmpegFailed {
                exit_code: None,
                stderr: "failed to acquire ffmpeg stdout pipe".into(),
            }
        })?;
        let stderr_drain = spawn_stderr_drain(child.stderr.take());
        let reader = BufReader::with_capacity(STREAM_READ_BUF_BYTES, stdout);

        Ok(Self {
            key,
            child,
            reader: Some(reader),
            stderr_drain,
            next_cursor: start_frame_index,
            frame_byte_len,
            scratch_skip_buf: Vec::new(),
            finished: false,
            waited: false,
        })
    }

    /// Reads the next frame (`self.next_cursor`) from the open pipe and advances the cursor by 1.
    ///
    /// Returns `Ok(None)` on clean EOF and marks the stream finished.
    /// Returns `Err(MediaError::FfmpegFailed)` if the subprocess exited with a non-zero status.
    pub fn read_next_frame(
        &mut self,
        pts_index: &PtsIndex,
    ) -> Result<Option<Rgba8Frame>, MediaError> {
        if self.finished {
            return Ok(None);
        }
        let Some(&pts) = pts_index.as_slice().get(self.next_cursor) else {
            self.finished = true;
            return Ok(None);
        };
        let Some(reader) = self.reader.as_mut() else {
            self.finished = true;
            return Ok(None);
        };

        let mut buf = vec![0u8; self.frame_byte_len];
        match reader.read_exact(&mut buf) {
            Ok(()) => {
                let frame = Rgba8Frame::new(
                    self.key.width,
                    self.key.height,
                    pts,
                    AlphaMode::Straight,
                    buf,
                )?;
                self.next_cursor += 1;
                Ok(Some(frame))
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                let (status, stderr) = self.finish_on_eof();
                if let Some(st) = status
                    && !st.success()
                {
                    return Err(MediaError::FfmpegFailed {
                        exit_code: st.code(),
                        stderr,
                    });
                }
                Ok(None)
            }
            Err(e) => {
                let _ = self.shutdown();
                Err(MediaError::Io(e))
            }
        }
    }

    /// Reads and discards `count` intermediate frames into a reusable scratch buffer
    /// without allocating `Rgba8Frame`s.
    ///
    /// Returns `Ok(true)` if all `count` frames were skipped, or `Ok(false)` on clean EOF.
    pub fn skip_frames(&mut self, count: usize) -> Result<bool, MediaError> {
        if count == 0 {
            return Ok(!self.finished);
        }
        if self.finished {
            return Ok(false);
        }
        if self.scratch_skip_buf.len() != self.frame_byte_len {
            self.scratch_skip_buf.resize(self.frame_byte_len, 0);
        }
        let Some(reader) = self.reader.as_mut() else {
            self.finished = true;
            return Ok(false);
        };

        for _ in 0..count {
            match reader.read_exact(&mut self.scratch_skip_buf) {
                Ok(()) => {
                    self.next_cursor += 1;
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    let (status, stderr) = self.finish_on_eof();
                    if let Some(st) = status
                        && !st.success()
                    {
                        return Err(MediaError::FfmpegFailed {
                            exit_code: st.code(),
                            stderr,
                        });
                    }
                    return Ok(false);
                }
                Err(e) => {
                    let _ = self.shutdown();
                    return Err(MediaError::Io(e));
                }
            }
        }
        Ok(true)
    }

    /// Returns the next expected `PtsIndex` frame index for this open stream.
    #[inline]
    pub fn next_cursor(&self) -> usize {
        self.next_cursor
    }

    /// Forcefully terminates the subprocess (if still running), reaps it via `wait()`,
    /// joins the stderr drain thread, and returns captured stderr text.
    pub fn shutdown(&mut self) -> String {
        self.reader.take();
        if !self.waited {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.waited = true;
        }
        self.finished = true;
        self.stderr_drain
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    }

    fn finish_on_eof(&mut self) -> (Option<ExitStatus>, String) {
        self.finished = true;
        self.reader.take();
        let status = if !self.waited {
            let res = self.child.wait().ok();
            self.waited = true;
            res
        } else {
            None
        };
        let stderr = self
            .stderr_drain
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        (status, stderr)
    }
}

impl Drop for StreamingVideoDecoder {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Debug)]
struct PoolEntry {
    decoder: StreamingVideoDecoder,
    last_used_seq: u64,
}

/// Bounded LRU pool of open [`StreamingVideoDecoder`] subprocesses keyed by `(path, width, height, fit)`.
#[derive(Debug)]
pub struct DecoderPool {
    config: DecoderPoolConfig,
    access_seq: u64,
    streams: HashMap<StreamKey, PoolEntry>,
    lru_order: BTreeMap<u64, StreamKey>,
    telemetry: DecoderPoolTelemetry,
}

impl DecoderPool {
    /// Creates a new decoder pool with the given configuration.
    pub fn new(config: DecoderPoolConfig) -> Self {
        Self {
            config,
            access_seq: 0,
            streams: HashMap::with_capacity(config.max_open_streams),
            lru_order: BTreeMap::new(),
            telemetry: DecoderPoolTelemetry::default(),
        }
    }

    /// Decodes frame `target_frame_index` from `path` at `(width, height, fit)`.
    ///
    /// Reuses an open stream on sequential reads or small forward skips (`<= max_sequential_skip`),
    /// and reopens via `-ss` seek on rewinds, large jumps, or unexpected EOF.
    pub fn decode_frame(
        &mut self,
        path: &Path,
        pts_index: &PtsIndex,
        target_frame_index: usize,
        width: u32,
        height: u32,
        fit: SourceFit,
    ) -> Result<Rgba8Frame, MediaError> {
        if width == 0 || height == 0 {
            return Err(MediaError::InvalidDimensions { width, height });
        }
        if pts_index.is_empty() || target_frame_index >= pts_index.len() {
            return Err(MediaError::EmptyMediaStream(format!(
                "cannot decode frame {target_frame_index} (len={}) for '{}'",
                pts_index.len(),
                path.display()
            )));
        }
        let expected_bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or(MediaError::InvalidDimensions { width, height })?;

        let key = StreamKey::new(path, width, height, fit);

        if self.config.max_open_streams == 0 {
            self.telemetry.spawns += 1;
            let mut decoder = StreamingVideoDecoder::open(key, pts_index, target_frame_index)?;
            return match decoder.read_next_frame(pts_index)? {
                Some(frame) => Ok(frame),
                None => {
                    self.telemetry.eof_reaches += 1;
                    drop(decoder);
                    decode_single_frame_fallback(
                        path,
                        pts_index,
                        target_frame_index,
                        width,
                        height,
                        fit,
                        expected_bytes,
                    )
                }
            };
        }

        enum Action {
            ReadSequential,
            SkipAndRead(usize),
            SeekReopen,
            SpawnNew,
        }

        let action = if let Some(entry) = self.streams.get(&key) {
            let cursor = entry.decoder.next_cursor();
            if entry.decoder.finished {
                Action::SeekReopen
            } else if target_frame_index == cursor {
                Action::ReadSequential
            } else if target_frame_index > cursor
                && target_frame_index - cursor <= self.config.max_sequential_skip
            {
                Action::SkipAndRead(target_frame_index - cursor)
            } else {
                Action::SeekReopen
            }
        } else {
            Action::SpawnNew
        };

        match action {
            Action::ReadSequential => {
                self.touch_entry(&key);
                self.telemetry.sequential_reads += 1;
                let read_res = self
                    .streams
                    .get_mut(&key)
                    .expect("entry verified present")
                    .decoder
                    .read_next_frame(pts_index);
                match read_res {
                    Ok(Some(frame)) => Ok(frame),
                    Ok(None) => {
                        self.telemetry.eof_reaches += 1;
                        self.remove_entry(&key);
                        decode_single_frame_fallback(
                            path,
                            pts_index,
                            target_frame_index,
                            width,
                            height,
                            fit,
                            expected_bytes,
                        )
                    }
                    Err(err) => {
                        self.remove_entry(&key);
                        Err(err)
                    }
                }
            }
            Action::SkipAndRead(delta) => {
                self.touch_entry(&key);
                let skip_and_read_res = {
                    let decoder = &mut self
                        .streams
                        .get_mut(&key)
                        .expect("entry verified present")
                        .decoder;
                    let before_cursor = decoder.next_cursor();
                    let skip_res = decoder.skip_frames(delta);
                    let skipped_now = decoder.next_cursor().saturating_sub(before_cursor);
                    (skipped_now, skip_res)
                };
                self.telemetry.skipped_frames += skip_and_read_res.0 as u64;
                match skip_and_read_res.1 {
                    Ok(true) => {
                        let read_res = self
                            .streams
                            .get_mut(&key)
                            .expect("entry verified present")
                            .decoder
                            .read_next_frame(pts_index);
                        match read_res {
                            Ok(Some(frame)) => Ok(frame),
                            Ok(None) => {
                                self.telemetry.eof_reaches += 1;
                                self.remove_entry(&key);
                                decode_single_frame_fallback(
                                    path,
                                    pts_index,
                                    target_frame_index,
                                    width,
                                    height,
                                    fit,
                                    expected_bytes,
                                )
                            }
                            Err(err) => {
                                self.remove_entry(&key);
                                Err(err)
                            }
                        }
                    }
                    Ok(false) => {
                        self.telemetry.eof_reaches += 1;
                        self.remove_entry(&key);
                        decode_single_frame_fallback(
                            path,
                            pts_index,
                            target_frame_index,
                            width,
                            height,
                            fit,
                            expected_bytes,
                        )
                    }
                    Err(err) => {
                        self.remove_entry(&key);
                        Err(err)
                    }
                }
            }
            Action::SeekReopen => {
                self.remove_entry(&key);
                self.telemetry.seek_reopens += 1;
                self.open_and_read_into_pool(
                    key,
                    path,
                    pts_index,
                    target_frame_index,
                    width,
                    height,
                    fit,
                    expected_bytes,
                )
            }
            Action::SpawnNew => {
                while self.streams.len() >= self.config.max_open_streams {
                    if self.evict_oldest_stream() {
                        self.telemetry.lru_evictions += 1;
                    } else {
                        break;
                    }
                }
                self.telemetry.spawns += 1;
                self.open_and_read_into_pool(
                    key,
                    path,
                    pts_index,
                    target_frame_index,
                    width,
                    height,
                    fit,
                    expected_bytes,
                )
            }
        }
    }

    /// Closes and drops all open decoder subprocesses associated with `path`.
    pub fn invalidate_path(&mut self, path: &Path) {
        let keys_to_remove: Vec<StreamKey> = self
            .streams
            .keys()
            .filter(|k| k.path == path)
            .cloned()
            .collect();
        for key in keys_to_remove {
            self.remove_entry(&key);
        }
    }

    /// Closes and reaps all open decoder subprocesses in the pool.
    pub fn close_all(&mut self) {
        self.lru_order.clear();
        self.streams.clear();
        self.telemetry.active_streams = 0;
    }

    /// Returns a snapshot of the pool's telemetry counters.
    pub fn telemetry(&self) -> DecoderPoolTelemetry {
        let mut tel = self.telemetry;
        tel.active_streams = self.streams.len();
        tel
    }

    #[allow(clippy::too_many_arguments)]
    fn open_and_read_into_pool(
        &mut self,
        key: StreamKey,
        path: &Path,
        pts_index: &PtsIndex,
        target_frame_index: usize,
        width: u32,
        height: u32,
        fit: SourceFit,
        expected_bytes: usize,
    ) -> Result<Rgba8Frame, MediaError> {
        let mut decoder = StreamingVideoDecoder::open(key.clone(), pts_index, target_frame_index)?;
        match decoder.read_next_frame(pts_index)? {
            Some(frame) => {
                self.insert_entry(key, decoder);
                Ok(frame)
            }
            None => {
                self.telemetry.eof_reaches += 1;
                drop(decoder);
                decode_single_frame_fallback(
                    path,
                    pts_index,
                    target_frame_index,
                    width,
                    height,
                    fit,
                    expected_bytes,
                )
            }
        }
    }

    fn touch_entry(&mut self, key: &StreamKey) {
        if let Some(entry) = self.streams.get_mut(key) {
            let old_seq = entry.last_used_seq;
            self.lru_order.remove(&old_seq);
            self.access_seq += 1;
            let new_seq = self.access_seq;
            entry.last_used_seq = new_seq;
            self.lru_order.insert(new_seq, key.clone());
        }
    }

    fn insert_entry(&mut self, key: StreamKey, decoder: StreamingVideoDecoder) {
        if let Some(old) = self.streams.remove(&key) {
            self.lru_order.remove(&old.last_used_seq);
        }
        self.access_seq += 1;
        let seq = self.access_seq;
        self.lru_order.insert(seq, key.clone());
        self.streams.insert(
            key,
            PoolEntry {
                decoder,
                last_used_seq: seq,
            },
        );
        self.telemetry.active_streams = self.streams.len();
    }

    fn remove_entry(&mut self, key: &StreamKey) {
        if let Some(entry) = self.streams.remove(key) {
            self.lru_order.remove(&entry.last_used_seq);
        }
        self.telemetry.active_streams = self.streams.len();
    }

    fn evict_oldest_stream(&mut self) -> bool {
        let Some((&oldest_seq, oldest_key)) = self.lru_order.iter().next() else {
            return false;
        };
        let oldest_key = oldest_key.clone();
        self.lru_order.remove(&oldest_seq);
        let removed = self.streams.remove(&oldest_key).is_some();
        self.telemetry.active_streams = self.streams.len();
        removed
    }
}

impl Default for DecoderPool {
    fn default() -> Self {
        Self::new(DecoderPoolConfig::default())
    }
}

impl Drop for DecoderPool {
    fn drop(&mut self) {
        self.close_all();
    }
}

fn decode_single_frame_fallback(
    path: &Path,
    pts_index: &PtsIndex,
    target_frame_index: usize,
    width: u32,
    height: u32,
    fit: SourceFit,
    expected_bytes: usize,
) -> Result<Rgba8Frame, MediaError> {
    let target_tick = pts_index.as_slice()[target_frame_index];
    match FfmpegBridge::decode_video_frame_with_fit(path, pts_index, target_tick, width, height, fit)
    {
        Ok(frame) => Ok(frame),
        Err(MediaError::BufferLengthMismatch { actual, .. }) => {
            Err(MediaError::BufferLengthMismatch {
                expected: expected_bytes,
                actual,
            })
        }
        Err(err) => Err(err),
    }
}

pub(crate) fn spawn_stderr_drain(stderr: Option<ChildStderr>) -> Option<JoinHandle<String>> {
    let mut stderr = stderr?;
    Some(std::thread::spawn(move || {
        let mut captured = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match stderr.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if captured.len() < MAX_STDERR_CAPTURE_BYTES {
                        let remaining = MAX_STDERR_CAPTURE_BYTES - captured.len();
                        let take = n.min(remaining);
                        captured.extend_from_slice(&buf[..take]);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        String::from_utf8_lossy(&captured).trim().to_owned()
    }))
}
