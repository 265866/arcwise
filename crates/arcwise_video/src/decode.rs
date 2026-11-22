//! The decode thread.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{ptr, slice};

use crossbeam_channel::{Receiver, Select, Sender};
use windows::Win32::Foundation::{self, E_FAIL, HANDLE, HMODULE, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    self, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_FENCE_FLAG_NONE, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    ID3D11Device, ID3D11Device5, ID3D11DeviceContext, ID3D11DeviceContext4, ID3D11Fence,
    ID3D11Multithread, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::{DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET};
use windows::Win32::Media::MediaFoundation::{
    self, IMF2DBuffer2, IMFDXGIBuffer, IMFDXGIDeviceManager, IMFSample, IMFSourceReader,
    MF_E_INVALID_STREAM_DATA, MF_E_INVALIDMEDIATYPE, MF_E_TOPO_CODEC_NOT_FOUND, MF_LOW_LATENCY,
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION,
    MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_D3D_MANAGER, MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READER_MEDIASOURCE,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION,
    MF2DBuffer_LockFlags_Read, MFMediaType_Video, MFSTARTUP_NOSOCKET, MFVideoFormat_ARGB32,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{self, COINIT_MULTITHREADED};
use windows::Win32::System::Threading;
use windows::Win32::System::Variant::VT_I8;
use windows::core::{self, GUID, HSTRING, Interface};

use crate::command::Command;
use crate::event::Event;
use crate::frame::Frame;

/// The reader's first video stream, the only one it reads.
const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0.cast_unsigned();

/// How long to wait for the GPU to finish copying a frame before giving up on it.
const COPY_TIMEOUT_MS: u32 = 1000;

/// How long a request to prepare waits for the frame in hand to be taken; see [`Clip::send`].
const PREPARE_WAIT: Duration = Duration::from_millis(50);

/// Runs the decode thread until the [`Decoder`](crate::Decoder) is dropped.
///
/// Without Media Foundation, every request to open is answered with the failure, so the player
/// shows it instead of waiting.
#[expect(
    unsafe_code,
    reason = "COM and Media Foundation are started and shut down through their C entry points, \
              which have no safe binding"
)]
pub(crate) fn run(commands: Receiver<Command>, frames: &Sender<Frame>, events: &Sender<Event>) {
    // SAFETY: called once at the start of this thread; a successful call is balanced below.
    let com = unsafe { Com::CoInitializeEx(None, COINIT_MULTITHREADED) }.ok();
    let started = com.clone().and_then(|()| {
        // SAFETY: called once, after COM started on this thread; a successful call is balanced
        // below.
        unsafe { MediaFoundation::MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) }
    });
    match &started {
        Ok(()) => decode(&commands, frames, events),
        Err(e) => {
            for command in commands {
                if let Command::Open { generation, .. } = command {
                    let failed = Event::Failed {
                        generation,
                        error: format!("Media Foundation is not available: {}", e.message()),
                        codec_missing: false,
                    };
                    if events.send(failed).is_err() {
                        break;
                    }
                }
            }
        }
    }
    if started.is_ok() {
        // SAFETY: balances the successful startup above; nothing of Media Foundation's is alive
        // any more.
        let shutdown = unsafe { MediaFoundation::MFShutdown() };
        debug_assert!(
            shutdown.is_ok(),
            "Media Foundation shuts down once for its one startup: {shutdown:?}"
        );
    }
    if com.is_ok() {
        // SAFETY: balances the successful COM initialization above, after Media Foundation shut
        // down.
        unsafe { Com::CoUninitialize() };
    }
}

/// Answers commands until the decoder is dropped, which an event that fails to send also shows.
/// Without a usable GPU, decodes in software rather than not at all.
fn decode(commands: &Receiver<Command>, frames: &Sender<Frame>, events: &Sender<Event>) {
    let mut device = Device::try_new().ok();
    let mut ready: Option<Prepared> = None;
    let mut next = commands.recv().ok();
    while let Some(command) = next.take() {
        next = match latest(commands, command) {
            Command::Stop | Command::Seek { .. } => commands.recv().ok(),
            Command::Prepare { path, max_height } => {
                ready = match prepare(device.as_mut(), path, max_height) {
                    Ok(prepared) => Some(prepared),
                    Err(e) => {
                        recover(&mut device, &mut ready, &e);
                        None
                    }
                };
                commands.recv().ok()
            }
            Command::Open {
                generation,
                path,
                max_height,
            } => {
                let prepared = match ready.take() {
                    Some(p) if p.path == path && p.max_height == max_height => Ok(p),
                    _ => prepare(device.as_mut(), path.clone(), max_height),
                };
                let mut clip = Clip {
                    generation,
                    device: device.as_mut(),
                    ready: &mut ready,
                    waiting: None,
                    commands,
                    frames,
                    events,
                };
                match prepared.and_then(|prepared| clip.play(prepared)) {
                    Ok(command) => command,
                    Err(e) => {
                        let sent = events.send(Event::Failed {
                            generation: clip.generation,
                            error: format!("{}: {}", path.display(), e.message()),
                            codec_missing: e.code() == MF_E_TOPO_CODEC_NOT_FOUND,
                        });
                        recover(&mut device, &mut ready, &e);
                        if sent.is_ok() {
                            commands.recv().ok()
                        } else {
                            None
                        }
                    }
                }
            }
        };
    }
}

/// Skips requests to prepare that newer commands have overtaken, with no clip playing: only
/// the latest prepare counts, an open or stop queued behind it wins, and a seek, which has
/// nothing to move, is passed over. A seek or stop that comes through finds no clip playing,
/// since it stopped or failed to open, and does nothing.
fn latest(commands: &Receiver<Command>, mut command: Command) -> Command {
    while matches!(command, Command::Prepare { .. }) {
        match commands.try_recv() {
            Ok(Command::Seek { .. }) => {}
            Ok(newer) => command = newer,
            Err(_) => break,
        }
    }
    command
}

/// Whether `e` says the GPU was taken away, by a driver update or a GPU reset.
fn device_lost(e: &core::Error) -> bool {
    e.code() == DXGI_ERROR_DEVICE_REMOVED || e.code() == DXGI_ERROR_DEVICE_RESET
}

/// A driver update or GPU reset takes the device away; makes a new one for the next clip
/// rather than failing every clip until a restart, and drops the clip prepared on the old one.
fn recover(device: &mut Option<Device>, ready: &mut Option<Prepared>, e: &core::Error) {
    if device_lost(e) {
        *device = Device::try_new().ok();
        *ready = None;
    }
}

fn too_small(what: &str) -> core::Error {
    core::Error::new(
        MF_E_INVALIDMEDIATYPE,
        format!("the decoded {what} is smaller than the frame"),
    )
}

/// The size of a frame, in pixels.
struct FrameSize {
    width: u32,
    height: u32,
}

impl FrameSize {
    /// Reads a size packed as Media Foundation packs `MF_MT_FRAME_SIZE`: the width in the high
    /// 32 bits and the height in the low 32.
    fn unpack(packed: u64) -> FrameSize {
        let [w0, w1, w2, w3, h0, h1, h2, h3] = packed.to_be_bytes();
        FrameSize {
            width: u32::from_be_bytes([w0, w1, w2, w3]),
            height: u32::from_be_bytes([h0, h1, h2, h3]),
        }
    }

    /// Packs the size the way [`FrameSize::unpack`] reads it.
    fn pack(&self) -> u64 {
        (u64::from(self.width) << 32) | u64::from(self.height)
    }
}

/// A staging texture and the description of the decoded textures it was made to match.
struct Staging {
    texture: ID3D11Texture2D,
    matches: D3D11_TEXTURE2D_DESC,
}

/// The GPU side: decoding, color conversion and the copy back to memory.
struct Device {
    manager: IMFDXGIDeviceManager,
    direct3d: ID3D11Device,
    context: ID3D11DeviceContext,
    /// Reused while frames keep the same size and format.
    staging: Option<Staging>,
    fence: ID3D11Fence,
    fence_value: u64,
    fence_event: HANDLE,
}

impl Device {
    /// Makes a hardware Direct3D 11 device with video support, protected for use from Media
    /// Foundation's own worker threads, and a device manager that hands it to the decoders.
    #[expect(
        unsafe_code,
        reason = "Direct3D and Media Foundation are reached only through COM calls, which have \
                  no safe binding"
    )]
    fn try_new() -> core::Result<Device> {
        let mut device = None;
        // SAFETY: plain Direct3D call with a valid out-pointer for the device, which this
        // function then owns.
        unsafe {
            Direct3D11::D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&raw mut device),
                None,
                None,
            )?;
        }
        let device: ID3D11Device = device.ok_or_else(core::Error::empty)?;
        let multithread = device.cast::<ID3D11Multithread>()?;
        // SAFETY: a flag on a device this function owns; the previous setting it returns is of
        // no interest.
        let _previous = unsafe { multithread.SetMultithreadProtected(true) };
        let mut token = 0;
        let mut manager = None;
        // SAFETY: valid out-pointers for the token and the manager, which this function then
        // owns.
        unsafe { MediaFoundation::MFCreateDXGIDeviceManager(&raw mut token, &raw mut manager)? };
        let manager = manager.ok_or_else(core::Error::empty)?;
        // SAFETY: hands the manager a device that stays alive as long as the struct holding both,
        // with the token the manager came with.
        unsafe { manager.ResetDevice(&device, token)? };
        // SAFETY: a getter on a live device.
        let context = unsafe { device.GetImmediateContext()? };
        let device5 = device.cast::<ID3D11Device5>()?;
        let mut fence: Option<ID3D11Fence> = None;
        // SAFETY: a valid out-pointer for the fence, which the struct then owns.
        unsafe { device5.CreateFence(0, D3D11_FENCE_FLAG_NONE, &raw mut fence)? };
        // SAFETY: an unnamed auto-reset event with default security; the struct owns the handle
        // and closes it when dropped.
        let fence_event = unsafe { Threading::CreateEventW(None, false, false, None)? };
        Ok(Device {
            manager,
            direct3d: device,
            context,
            staging: None,
            fence: fence.ok_or_else(core::Error::empty)?,
            fence_value: 0,
            fence_event,
        })
    }

    /// Copies a decoded texture into memory through a staging texture.
    ///
    /// Waits for the copy in the kernel, on a fence, so `Map` finds the data ready rather than
    /// spinning in the driver. Media Foundation's own `Lock2D` waits with `Sleep`, whose
    /// granularity dropped playback to 20 fps on some machines.
    #[expect(
        unsafe_code,
        reason = "the decoded texture can be copied and mapped only through Direct3D's COM \
                  calls, and the mapped memory read only through a raw pointer"
    )]
    fn read_back(
        &mut self,
        buffer: &IMFDXGIBuffer,
        width: u32,
        height: u32,
    ) -> core::Result<Vec<u8>> {
        let mut raw = ptr::null_mut();
        // SAFETY: a valid out-pointer for the texture of the decoder's sample, which outlives
        // this call.
        unsafe { buffer.GetResource(&ID3D11Texture2D::IID, &raw mut raw)? };
        // SAFETY: GetResource succeeded, so `raw` holds a reference to an ID3D11Texture2D, the
        // interface asked for, which `texture` takes over.
        let texture = unsafe { ID3D11Texture2D::from_raw(raw) };
        // SAFETY: a getter on the sample's buffer.
        let subresource = unsafe { buffer.GetSubresourceIndex()? };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: a valid out-pointer for the description of a live texture.
        unsafe { texture.GetDesc(&raw mut desc) };
        if desc.Width < width || desc.Height < height {
            return Err(too_small("texture"));
        }
        let fits = self.staging.as_ref().is_some_and(|staging| {
            staging.matches.Width == desc.Width
                && staging.matches.Height == desc.Height
                && staging.matches.Format == desc.Format
        });
        if !fits {
            let staging_desc = D3D11_TEXTURE2D_DESC {
                MipLevels: 1,
                ArraySize: 1,
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0.cast_unsigned(),
                MiscFlags: 0,
                ..desc
            };
            let mut created = None;
            // SAFETY: a valid description and out-pointer; the struct owns the texture made.
            unsafe {
                self.direct3d.CreateTexture2D(
                    &raw const staging_desc,
                    None,
                    Some(&raw mut created),
                )?;
            }
            self.staging = Some(Staging {
                texture: created.ok_or_else(core::Error::empty)?,
                matches: desc,
            });
        }
        let Some(Staging {
            texture: target, ..
        }) = &self.staging
        else {
            return Err(core::Error::empty());
        };
        // SAFETY: the staging texture was made to match the decoded one in size and format, so
        // the whole subresource fits.
        unsafe {
            self.context
                .CopySubresourceRegion(target, 0, 0, 0, 0, &texture, subresource, None);
        }
        self.fence_value += 1;
        let context4 = self.context.cast::<ID3D11DeviceContext4>()?;
        // SAFETY: signals a fence the struct owns on its own device's context.
        unsafe { context4.Signal(&self.fence, self.fence_value)? };
        // SAFETY: the event is the struct's own, open until it is dropped.
        unsafe {
            self.fence
                .SetEventOnCompletion(self.fence_value, self.fence_event)?;
        }
        // SAFETY: flushes the struct's own context.
        unsafe { self.context.Flush() };
        // SAFETY: waits on the struct's own event, open until it is dropped.
        let waited = unsafe { Threading::WaitForSingleObject(self.fence_event, COPY_TIMEOUT_MS) };
        if waited != WAIT_OBJECT_0 {
            return Err(core::Error::new(
                E_FAIL,
                "the GPU did not finish copying a frame",
            ));
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: maps the staging texture, made with CPU read access, with a valid out-pointer.
        unsafe {
            self.context
                .Map(target, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped))?;
        }
        let row = width as usize * 4;
        let pitch = mapped.RowPitch as usize;
        if pitch < row {
            // SAFETY: unmaps the subresource mapped above.
            unsafe { self.context.Unmap(target, 0) };
            return Err(too_small("mapped row"));
        }
        let mut pixels = vec![0u8; row * height as usize];
        let mut source = mapped.pData.cast::<u8>().cast_const();
        for out in pixels.chunks_exact_mut(row) {
            // SAFETY: the texture is mapped, and at least `height` rows tall, each `pitch` bytes
            // after the last and at least `row` bytes long, so this row lies inside the mapped
            // memory, which nothing writes while it is mapped.
            out.copy_from_slice(unsafe { slice::from_raw_parts(source, row) });
            source = source.wrapping_add(pitch);
        }
        // SAFETY: unmaps the subresource mapped above, after its last read.
        unsafe { self.context.Unmap(target, 0) };
        Ok(pixels)
    }
}

impl Drop for Device {
    #[expect(
        unsafe_code,
        reason = "the fence's event is a kernel handle, closed only through its C entry point"
    )]
    fn drop(&mut self) {
        // SAFETY: the event was created by this struct and nothing waits on it any more.
        let closed = unsafe { Foundation::CloseHandle(self.fence_event) };
        debug_assert!(
            closed.is_ok(),
            "the fence event this device made closes once: {closed:?}"
        );
    }
}

/// A clip opened with its first frame decoded and copied back, ready to play at once.
struct Prepared {
    path: PathBuf,
    max_height: Option<u32>,
    reader: IMFSourceReader,
    width: u32,
    height: u32,
    duration: Duration,
    /// The first frame's time.
    pts: Duration,
    /// The first frame's pixels.
    bgra: Vec<u8>,
}

/// Opens a clip and decodes its first frame. This is most of the wait before a clip shows, so
/// the player asks for it ahead of time for clips it is likely to play next.
fn prepare(
    mut device: Option<&mut Device>,
    path: PathBuf,
    max_height: Option<u32>,
) -> core::Result<Prepared> {
    let reader = open(device.as_deref(), &path, max_height)?;
    let Format {
        mut width,
        mut height,
        duration,
    } = output_format(&reader)?;
    loop {
        let read = read(&reader)?;
        if read.flags & MF_SOURCE_READERF_ENDOFSTREAM.0.cast_unsigned() != 0 {
            return Err(core::Error::new(
                MF_E_INVALID_STREAM_DATA,
                "the clip has no frames",
            ));
        }
        if read.flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0.cast_unsigned() != 0 {
            let format = output_format(&reader)?;
            width = format.width;
            height = format.height;
        }
        let Some(sample) = read.sample else { continue };
        let pixels = pixels(device.as_deref_mut(), &sample, width, height)?;
        return Ok(Prepared {
            path,
            max_height,
            reader,
            width,
            height,
            duration,
            pts: presentation_time(read.timestamp),
            bgra: pixels,
        });
    }
}

/// What one `ReadSample` returned.
struct Read {
    /// The `MF_SOURCE_READERF_*` flags.
    flags: u32,
    /// The sample's time, in 100 ns units.
    timestamp: i64,
    /// The sample, if any.
    sample: Option<IMFSample>,
}

/// Reads the next sample.
#[expect(
    unsafe_code,
    reason = "the source reader is reached only through its COM methods, which have no safe \
              binding"
)]
fn read(reader: &IMFSourceReader) -> core::Result<Read> {
    let mut flags = 0u32;
    let mut timestamp = 0i64;
    let mut sample = None;
    // SAFETY: valid out-pointers for the duration of the call.
    unsafe {
        reader.ReadSample(
            VIDEO,
            0,
            None,
            Some(&raw mut flags),
            Some(&raw mut timestamp),
            Some(&raw mut sample),
        )?;
    }
    Ok(Read {
        flags,
        timestamp,
        sample,
    })
}

/// A time in Media Foundation's 100 ns units as a duration, with times before the start as the
/// start.
fn presentation_time(timestamp: i64) -> Duration {
    Duration::from_nanos(u64::try_from(timestamp).unwrap_or(0).saturating_mul(100))
}

/// Whether `sample`, which starts at `start`, is over before `position`.
#[expect(
    unsafe_code,
    reason = "a sample's duration is read only through its COM method, which has no safe binding"
)]
fn ends_before(sample: &IMFSample, start: Duration, position: Duration) -> bool {
    // SAFETY: reads a property of a sample the reader returned.
    let length = unsafe { sample.GetSampleDuration() }.map_or(Duration::ZERO, presentation_time);
    if length.is_zero() {
        start < position
    } else {
        start + length <= position
    }
}

/// The pixels of `sample`, through the GPU when it decoded there.
#[expect(
    unsafe_code,
    reason = "a sample's buffer is reached only through its COM method, which has no safe binding"
)]
fn pixels(
    device: Option<&mut Device>,
    sample: &IMFSample,
    width: u32,
    height: u32,
) -> core::Result<Vec<u8>> {
    // SAFETY: the buffer belongs to `sample`, which outlives this call.
    let buffer = unsafe { sample.GetBufferByIndex(0)? };
    if let (Some(device), Ok(dxgi)) = (device, buffer.cast::<IMFDXGIBuffer>()) {
        return device.read_back(&dxgi, width, height);
    }
    lock_and_copy(&buffer.cast::<IMF2DBuffer2>()?, width, height)
}

/// A clip asked to be prepared while another plays.
struct Waiting {
    path: PathBuf,
    max_height: Option<u32>,
    /// Since when it waits.
    since: Instant,
}

/// One clip being played.
struct Clip<'thread> {
    /// The generation of the open, or of the last seek.
    generation: u64,
    device: Option<&'thread mut Device>,
    /// Where a clip prepared while this one plays is kept.
    ready: &'thread mut Option<Prepared>,
    waiting: Option<Waiting>,
    commands: &'thread Receiver<Command>,
    frames: &'thread Sender<Frame>,
    events: &'thread Sender<Event>,
}

/// Why a clip stopped playing.
enum Ended {
    /// A command other than a prepare arrived.
    Command(Command),
    /// The decoder was dropped.
    Dropped,
}

impl Clip<'_> {
    /// Plays the clip in a loop until a command other than a prepare arrives, and returns that
    /// command; `None` when the decoder has been dropped.
    ///
    /// After a seek, frames before the position asked for are decoded and dropped, not copied
    /// back, since the reader can only start at a key frame. A prepare waits while a seek skips
    /// ahead, so the seek's frame goes first, as a clip just opened sends its first frame before
    /// a prepare; another seek, an open or a stop cuts the skip short. A seek past the last
    /// frame, as when the reported duration overstates the stream, shows the first. A clip can
    /// change size partway; frames from there on have the new one. A whole pass without a frame
    /// fails the clip, since nothing will ever play.
    fn play(&mut self, prepared: Prepared) -> core::Result<Option<Command>> {
        let Prepared {
            reader,
            mut width,
            mut height,
            duration,
            pts,
            bgra,
            ..
        } = prepared;
        let opened = Event::Opened {
            generation: self.generation,
            width,
            height,
            duration,
        };
        if self.events.send(opened).is_err() {
            return Ok(None);
        }
        let mut frame = Frame {
            generation: self.generation,
            pts,
            width,
            height,
            bgra,
        };
        let mut loop_offset = Duration::ZERO;
        let mut empty_loops = 0;
        let mut skip_to: Option<Duration> = None;
        loop {
            let mut command = match self.send(frame)? {
                Some(Ended::Command(command)) => Some(command),
                Some(Ended::Dropped) => return Ok(None),
                None => None,
            };
            frame = loop {
                if command.is_none() {
                    command = match skip_to {
                        None => self.answer()?,
                        Some(_) => self.commands.try_recv().ok().and_then(|c| self.wait(c)),
                    };
                }
                match command.take() {
                    Some(Command::Seek {
                        generation,
                        position,
                    }) => {
                        self.generation = generation;
                        loop_offset = Duration::ZERO;
                        empty_loops = 0;
                        set_position(&reader, position)?;
                        skip_to = Some(position);
                        if let Some(waiting) = &mut self.waiting {
                            waiting.since = Instant::now();
                        }
                        continue;
                    }
                    Some(other) => return Ok(Some(other)),
                    None => {}
                }
                let read = read(&reader)?;
                if read.flags & MF_SOURCE_READERF_ENDOFSTREAM.0.cast_unsigned() != 0 {
                    empty_loops += 1;
                    if empty_loops > 1 {
                        return Err(core::Error::new(
                            MF_E_INVALID_STREAM_DATA,
                            "the clip has no frames",
                        ));
                    }
                    skip_to = None;
                    loop_offset += duration;
                    rewind(&reader)?;
                    continue;
                }
                if read.flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0.cast_unsigned() != 0 {
                    let format = output_format(&reader)?;
                    width = format.width;
                    height = format.height;
                }
                let Some(sample) = read.sample else { continue };
                empty_loops = 0;
                let start = presentation_time(read.timestamp);
                if skip_to.is_some_and(|position| ends_before(&sample, start, position)) {
                    continue;
                }
                skip_to = None;
                break Frame {
                    generation: self.generation,
                    pts: loop_offset + start,
                    width,
                    height,
                    bgra: pixels(self.device.as_deref_mut(), &sample, width, height)?,
                };
            };
        }
    }

    /// Waits for the player to take `frame`, answering commands meanwhile. Returns why the clip
    /// ended, if it did. A request to prepare waits a little for the frame to go first: a clip
    /// just opened sends its first frame within a display frame or two, and must not wait
    /// behind a prepare. A paused or hidden clip sends none, so the prepare goes ahead then.
    fn send(&mut self, frame: Frame) -> core::Result<Option<Ended>> {
        let mut frame = Some(frame);
        loop {
            let mut select = Select::new();
            let send = select.send(self.frames);
            select.recv(self.commands);
            let deadline = self
                .waiting
                .as_ref()
                .map(|waiting| waiting.since + PREPARE_WAIT);
            let operation = match deadline {
                None => select.select(),
                Some(deadline) => {
                    let Ok(operation) = select.select_deadline(deadline) else {
                        if let Some(command) = self.answer()? {
                            return Ok(Some(Ended::Command(command)));
                        }
                        continue;
                    };
                    operation
                }
            };
            if operation.index() == send {
                let frame = frame.take().expect("a frame is sent once");
                if operation.send(self.frames, frame).is_err() {
                    return Ok(Some(Ended::Dropped));
                }
                return Ok(self.answer()?.map(Ended::Command));
            }
            match operation.recv(self.commands) {
                Ok(command) => {
                    if let Some(command) = self.wait(command) {
                        return Ok(Some(Ended::Command(command)));
                    }
                }
                Err(_) => return Ok(Some(Ended::Dropped)),
            }
        }
    }

    /// Keeps a request to prepare for later, in place of any before it, and hands back any
    /// other command. The request outlives a seek, which keeps this clip playing, but not an
    /// open or stop, which end it.
    fn wait(&mut self, command: Command) -> Option<Command> {
        let Command::Prepare { path, max_height } = command else {
            return Some(command);
        };
        let since = self
            .waiting
            .take()
            .map_or_else(Instant::now, |waiting| waiting.since);
        self.waiting = Some(Waiting {
            path,
            max_height,
            since,
        });
        None
    }

    /// Prepares the clip waiting to be, between frames of this one, unless a command queued
    /// meanwhile comes first, which it hands back. A lost device fails the playing clip too,
    /// which makes a new one.
    fn answer(&mut self) -> core::Result<Option<Command>> {
        while let Ok(command) = self.commands.try_recv() {
            if let Some(command) = self.wait(command) {
                return Ok(Some(command));
            }
        }
        let Some(Waiting {
            path, max_height, ..
        }) = self.waiting.take()
        else {
            return Ok(None);
        };
        *self.ready = match prepare(self.device.as_deref_mut(), path, max_height) {
            Ok(prepared) => Some(prepared),
            Err(e) if device_lost(&e) => return Err(e),
            Err(_) => None,
        };
        Ok(None)
    }
}

/// Opens a source reader for `path` that converts the video to BGRA, on the GPU when there is
/// a device, scaled down to `max_height` rows when that is smaller than the clip.
///
/// Asks for ARGB32 rather than RGB32, since the video processor then fills in alpha, and the UI
/// draws with it. Turns on low latency, which holds fewer frames inside the decoder: about 12 MB
/// less per clip open. A scaled size is rounded down to even numbers, since the video processor
/// rejects odd ones for some formats.
#[expect(
    unsafe_code,
    reason = "a source reader is made and configured only through Media Foundation's COM calls, \
              which have no safe binding"
)]
fn open(
    device: Option<&Device>,
    path: &Path,
    max_height: Option<u32>,
) -> core::Result<IMFSourceReader> {
    let mut attributes = None;
    // SAFETY: a valid out-pointer for the attribute store, which this function then owns.
    unsafe { MediaFoundation::MFCreateAttributes(&raw mut attributes, 3)? };
    let attributes = attributes.ok_or_else(core::Error::empty)?;
    if let Some(device) = device {
        // SAFETY: stores a reference to the device manager, which the reader keeps.
        unsafe { attributes.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, &device.manager)? };
        // SAFETY: sets a flag on the attribute store this function owns.
        unsafe { attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)? };
    }
    // SAFETY: as above.
    unsafe { attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)? };
    // SAFETY: as above.
    unsafe { attributes.SetUINT32(&MF_LOW_LATENCY, 1)? };
    let url = HSTRING::from(path.as_os_str());
    // SAFETY: the URL and attributes outlive the call; the reader it returns owns everything it
    // needs, including a reference to the device manager.
    let reader = unsafe { MediaFoundation::MFCreateSourceReaderFromURL(&url, &attributes)? };
    // SAFETY: selects streams on the reader this function owns.
    unsafe { reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0.cast_unsigned(), false)? };
    // SAFETY: as above.
    unsafe { reader.SetStreamSelection(VIDEO, true)? };
    // SAFETY: makes an empty media type, which this function then owns.
    let output = unsafe { MediaFoundation::MFCreateMediaType()? };
    // SAFETY: sets a GUID on the media type this function owns.
    unsafe { output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)? };
    // SAFETY: as above.
    unsafe { output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_ARGB32)? };
    // SAFETY: a getter on the reader this function owns.
    let native_type = unsafe { reader.GetNativeMediaType(VIDEO, 0)? };
    // SAFETY: a getter on the native media type just returned.
    let native = FrameSize::unpack(unsafe { native_type.GetUINT64(&MF_MT_FRAME_SIZE)? });
    if let Some(max) = max_height.filter(|&max| max > 0 && max < native.height) {
        let width = u64::from(native.width) * u64::from(max) / u64::from(native.height);
        let scaled = FrameSize {
            width: u32::try_from(width)
                .expect("a width scaled by less than one is smaller than the native one")
                & !1,
            height: max & !1,
        };
        // SAFETY: sets a size on the media type this function owns.
        unsafe { output.SetUINT64(&MF_MT_FRAME_SIZE, scaled.pack())? };
    }
    // SAFETY: the media type outlives the call, which copies what it needs.
    unsafe { reader.SetCurrentMediaType(VIDEO, None, &output)? };
    Ok(reader)
}

/// The frame size and duration a reader decodes to.
struct Format {
    width: u32,
    height: u32,
    duration: Duration,
}

/// Reads the frame size and duration of a configured reader.
#[expect(
    unsafe_code,
    reason = "the reader's media type and duration are read only through its COM methods and a \
              PROPVARIANT union, which have no safe binding"
)]
fn output_format(reader: &IMFSourceReader) -> core::Result<Format> {
    // SAFETY: a getter on a configured reader.
    let media_type = unsafe { reader.GetCurrentMediaType(VIDEO)? };
    // SAFETY: a getter on the media type just returned.
    let size = FrameSize::unpack(unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE)? });
    // SAFETY: a getter on a configured reader.
    let duration = unsafe {
        reader.GetPresentationAttribute(
            MF_SOURCE_READER_MEDIASOURCE.0.cast_unsigned(),
            &MF_PD_DURATION,
        )?
    };
    // SAFETY: the duration attribute is a PROPVARIANT tagged VT_UI8, so the tagged struct is the
    // union's live field.
    let tagged = unsafe { &duration.Anonymous.Anonymous };
    // SAFETY: as above, its value is the union's `uhVal` field.
    let duration = unsafe { tagged.Anonymous.uhVal };
    Ok(Format {
        width: size.width,
        height: size.height,
        duration: Duration::from_nanos(duration.saturating_mul(100)),
    })
}

fn rewind(reader: &IMFSourceReader) -> core::Result<()> {
    set_position(reader, Duration::ZERO)
}

/// Moves the reader to the key frame at or before `position`.
#[expect(
    unsafe_code,
    reason = "a reader's position is set only through its COM method, with a PROPVARIANT union, \
              which have no safe binding"
)]
fn set_position(reader: &IMFSourceReader, position: Duration) -> core::Result<()> {
    let ticks = i64::try_from(position.as_nanos() / 100).unwrap_or(i64::MAX);
    let mut at = PROPVARIANT::default();
    // SAFETY: a default PROPVARIANT is all zeros, VT_EMPTY, so its tagged struct is the union's
    // live field.
    let tagged = unsafe { &mut at.Anonymous.Anonymous };
    tagged.vt = VT_I8;
    tagged.Anonymous.hVal = ticks;
    // SAFETY: a PROPVARIANT tagged VT_I8 holding a time in 100 ns units, which is what
    // SetCurrentPosition expects with the null time format.
    unsafe { reader.SetCurrentPosition(&GUID::zeroed(), &raw const at) }
}

/// The software path: the frame is already in memory. The software video processor can leave
/// alpha at zero, so a frame whose first pixel has none is made opaque.
#[expect(
    unsafe_code,
    reason = "a decoded buffer is locked only through its COM methods, and read only through the \
              raw pointer they return"
)]
fn lock_and_copy(buffer: &IMF2DBuffer2, width: u32, height: u32) -> core::Result<Vec<u8>> {
    let row = width as usize * 4;
    let mut pixels = Vec::with_capacity(row * height as usize);
    let mut scanline0 = ptr::null_mut();
    let mut pitch = 0i32;
    let mut start = ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: valid out-pointers for the duration of the call; the buffer stays locked for
    // reading until the Unlock2D calls below.
    unsafe {
        buffer.Lock2DSize(
            MF2DBuffer_LockFlags_Read,
            &raw mut scanline0,
            &raw mut pitch,
            &raw mut start,
            &raw mut len,
        )?;
    }
    let first = scanline0.addr().wrapping_sub(start.addr()).cast_signed();
    let pitch = pitch as isize;
    if !rows_fit(first, pitch, row, height as usize, len as usize) {
        // SAFETY: unlocks the buffer locked above.
        unsafe { buffer.Unlock2D()? };
        return Err(too_small("buffer"));
    }
    let mut source = scanline0.cast_const();
    for _ in 0..height {
        // SAFETY: the buffer is locked for reading, and `rows_fit` has checked that every row
        // lies inside the locked range.
        pixels.extend_from_slice(unsafe { slice::from_raw_parts(source, row) });
        source = source.wrapping_offset(pitch);
    }
    // SAFETY: unlocks the buffer locked above, after its last read.
    unsafe { buffer.Unlock2D()? };
    if pixels.get(3) == Some(&0) {
        for px in pixels.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    }
    Ok(pixels)
}

/// Whether `rows` rows of `row` bytes, the first starting `first` bytes into a buffer of `len`
/// bytes and each `pitch` bytes after the last (negative for bottom-up images), all lie inside
/// the buffer. The first row's offset comes from the addresses, since the buffer may misreport,
/// and `offset_from` needs both pointers in one allocation.
fn rows_fit(first: isize, pitch: isize, row: usize, rows: usize, len: usize) -> bool {
    if rows == 0 {
        return true;
    }
    if pitch.unsigned_abs() < row {
        return false;
    }
    let Some(last) = isize::try_from(rows - 1)
        .ok()
        .and_then(|n| n.checked_mul(pitch))
        .and_then(|span| first.checked_add(span))
    else {
        return false;
    };
    let (low, high) = (first.min(last), first.max(last));
    low >= 0
        && usize::try_from(high)
            .ok()
            .and_then(|high| high.checked_add(row))
            .is_some_and(|end| end <= len)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four rows of 8 bytes, 16 apart: the last ends at 3 * 16 + 8 = 56.
    #[test]
    fn rows_must_lie_inside_the_buffer() {
        assert!(
            rows_fit(0, 16, 8, 4, 56),
            "the last row ends at the buffer's end"
        );
        assert!(
            !rows_fit(0, 16, 8, 4, 55),
            "the last row ends past the buffer"
        );
        assert!(!rows_fit(-1, 16, 8, 4, 100), "starts before the buffer");
        assert!(!rows_fit(0, 4, 8, 4, 100), "rows overlap");
    }

    /// The first row is the last in memory.
    #[test]
    fn bottom_up_rows_run_backwards() {
        assert!(
            rows_fit(48, -16, 8, 4, 56),
            "the rows run from 48 back to 0"
        );
        assert!(
            !rows_fit(40, -16, 8, 4, 56),
            "the last row starts before the buffer"
        );
    }

    #[test]
    fn no_rows_always_fit() {
        assert!(rows_fit(0, 0, 8, 0, 0), "no rows");
    }
}
