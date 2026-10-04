//! The decode thread.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, select};
use windows::Win32::Foundation::{HANDLE, HMODULE};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_I8;
use windows::core::{GUID, HSTRING, Interface};

use crate::{Command, Event, Frame};

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// How long to wait for the GPU to finish copying a frame before giving up on it.
const COPY_TIMEOUT_MS: u32 = 1000;

pub fn run(commands: Receiver<Command>, frames: Sender<Frame>, events: Sender<Event>) {
    // SAFETY: called once at the start of this thread, paired with the shutdown calls below.
    let started = unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .and_then(|()| MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET))
    };
    if let Err(e) = started {
        // Answer every request with the failure, so the player shows it instead of waiting.
        for command in commands {
            if let Command::Open { generation, .. } = command {
                let error = format!("Media Foundation is not available: {e}");
                let _ = events.send(Event::Failed { generation, error });
            }
        }
        return;
    }
    // Without a usable GPU, decode in software rather than not at all.
    let device = Device::new().ok();

    let mut next = commands.recv().ok();
    while let Some(command) = next.take() {
        let Command::Open {
            generation,
            path,
            max_height,
        } = command
        else {
            next = commands.recv().ok();
            continue;
        };
        let clip = Clip {
            generation,
            device: device.as_ref(),
            commands: &commands,
            frames: &frames,
            events: &events,
        };
        next = match clip.play(&path, max_height) {
            Ok(command) => command,
            Err(e) => {
                let error = format!("{}: {}", path.display(), e.message());
                let _ = events.send(Event::Failed { generation, error });
                commands.recv().ok()
            }
        };
    }

    drop(device);
    // SAFETY: balances the successful startup calls above; nothing of Media Foundation's is
    // alive any more.
    unsafe {
        let _ = MFShutdown();
        CoUninitialize();
    }
}

/// The GPU side: decoding, color conversion and the copy back to memory.
struct Device {
    manager: IMFDXGIDeviceManager,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    /// Reused while frames keep the same size and format.
    staging: RefCell<Option<(ID3D11Texture2D, D3D11_TEXTURE2D_DESC)>>,
    fence: ID3D11Fence,
    fence_value: Cell<u64>,
    fence_event: HANDLE,
}

impl Device {
    fn new() -> windows::core::Result<Device> {
        // SAFETY: plain Direct3D and Media Foundation calls with valid out-pointers; every
        // returned interface is owned by the struct.
        unsafe {
            let mut device = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )?;
            let device: ID3D11Device = device.ok_or_else(windows::core::Error::empty)?;
            // Media Foundation uses the device from its own worker threads.
            let _ = device
                .cast::<ID3D11Multithread>()?
                .SetMultithreadProtected(true);
            let mut token = 0;
            let mut manager = None;
            MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
            let manager = manager.ok_or_else(windows::core::Error::empty)?;
            manager.ResetDevice(&device, token)?;
            let context = device.GetImmediateContext()?;
            let mut fence: Option<ID3D11Fence> = None;
            device
                .cast::<ID3D11Device5>()?
                .CreateFence(0, D3D11_FENCE_FLAG_NONE, &mut fence)?;
            Ok(Device {
                manager,
                device,
                context,
                staging: RefCell::default(),
                fence: fence.ok_or_else(windows::core::Error::empty)?,
                fence_value: Cell::default(),
                fence_event: CreateEventW(None, false, false, None)?,
            })
        }
    }

    /// Copies a decoded texture into memory through a staging texture.
    fn read_back(
        &self,
        buffer: &IMFDXGIBuffer,
        width: u32,
        height: u32,
    ) -> windows::core::Result<Vec<u8>> {
        // SAFETY: the texture comes from the decoder's sample, which outlives this call; the
        // staging texture is created to match it, and the mapped memory is read only between
        // Map and Unmap, within the row pitch the driver reports.
        unsafe {
            let mut raw = std::ptr::null_mut();
            buffer.GetResource(&ID3D11Texture2D::IID, &mut raw)?;
            let texture = ID3D11Texture2D::from_raw(raw);
            let subresource = buffer.GetSubresourceIndex()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            let mut staging = self.staging.borrow_mut();
            let fits = matches!(&*staging, Some((_, d))
                if d.Width == desc.Width && d.Height == desc.Height && d.Format == desc.Format);
            if !fits {
                let staging_desc = D3D11_TEXTURE2D_DESC {
                    MipLevels: 1,
                    ArraySize: 1,
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                    ..desc
                };
                let mut created = None;
                self.device
                    .CreateTexture2D(&staging_desc, None, Some(&mut created))?;
                *staging = Some((created.ok_or_else(windows::core::Error::empty)?, desc));
            }
            let Some((target, _)) = staging.as_ref() else {
                unreachable!("created above");
            };
            self.context
                .CopySubresourceRegion(target, 0, 0, 0, 0, &texture, subresource, None);
            // Wait for the copy in the kernel, so Map finds the data ready rather than spinning
            // in the driver. Media Foundation's own Lock2D waits with Sleep, whose granularity
            // dropped playback to 20 fps on some machines.
            let value = self.fence_value.get() + 1;
            self.fence_value.set(value);
            self.context
                .cast::<ID3D11DeviceContext4>()?
                .Signal(&self.fence, value)?;
            self.fence.SetEventOnCompletion(value, self.fence_event)?;
            self.context.Flush();
            WaitForSingleObject(self.fence_event, COPY_TIMEOUT_MS);

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(target, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let row = width as usize * 4;
            let mut pixels = vec![0u8; row * height as usize];
            let base = mapped.pData.cast::<u8>().cast_const();
            for (y, out) in pixels.chunks_exact_mut(row).enumerate() {
                out.copy_from_slice(std::slice::from_raw_parts(
                    base.add(y * mapped.RowPitch as usize),
                    row,
                ));
            }
            self.context.Unmap(target, 0);
            Ok(pixels)
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: the event was created by this struct and nothing waits on it any more.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.fence_event);
        }
    }
}

/// One clip being played.
struct Clip<'a> {
    generation: u64,
    device: Option<&'a Device>,
    commands: &'a Receiver<Command>,
    frames: &'a Sender<Frame>,
    events: &'a Sender<Event>,
}

impl Clip<'_> {
    /// Plays the clip in a loop until a command arrives, and returns that command; `None` when
    /// the decoder has been dropped.
    fn play(&self, path: &Path, max_height: Option<u32>) -> windows::core::Result<Option<Command>> {
        let reader = open(self.device, path, max_height)?;
        let (width, height, duration) = output_format(&reader)?;
        let _ = self.events.send(Event::Opened {
            generation: self.generation,
            width,
            height,
            duration,
        });
        let mut loop_offset = Duration::ZERO;
        let mut empty_loops = 0;
        loop {
            if let Ok(command) = self.commands.try_recv() {
                return Ok(Some(command));
            }
            let mut flags = 0u32;
            let mut timestamp = 0i64;
            let mut sample = None;
            // SAFETY: valid out-pointers for the duration of the call.
            unsafe {
                reader.ReadSample(
                    VIDEO,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut timestamp),
                    Some(&mut sample),
                )?;
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                empty_loops += 1;
                if empty_loops > 1 {
                    // A whole pass without a frame: nothing will ever play.
                    return Err(windows::core::Error::new(
                        MF_E_INVALID_STREAM_DATA,
                        "the clip has no frames",
                    ));
                }
                loop_offset += duration;
                rewind(&reader)?;
                continue;
            }
            let Some(sample) = sample else { continue };
            empty_loops = 0;
            let pts = loop_offset + Duration::from_nanos(timestamp.max(0) as u64 * 100);
            let bgra = self.pixels(&sample, width, height)?;
            let frame = Frame {
                generation: self.generation,
                pts,
                width,
                height,
                bgra,
            };
            // Wait for the player to take the frame, but answer a new command at once.
            select! {
                send(self.frames, frame) -> sent => if sent.is_err() { return Ok(None) },
                recv(self.commands) -> command => return Ok(command.ok()),
            }
        }
    }

    fn pixels(
        &self,
        sample: &IMFSample,
        width: u32,
        height: u32,
    ) -> windows::core::Result<Vec<u8>> {
        // SAFETY: the buffer belongs to `sample`, which outlives this call.
        let buffer = unsafe { sample.GetBufferByIndex(0)? };
        if let (Some(device), Ok(dxgi)) = (self.device, buffer.cast::<IMFDXGIBuffer>()) {
            return device.read_back(&dxgi, width, height);
        }
        lock_and_copy(&buffer.cast::<IMF2DBuffer2>()?, width, height)
    }
}

fn open(
    device: Option<&Device>,
    path: &Path,
    max_height: Option<u32>,
) -> windows::core::Result<IMFSourceReader> {
    // SAFETY: Media Foundation calls with valid arguments; the returned reader owns everything
    // it needs, including a reference to the device manager.
    unsafe {
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 3)?;
        let attributes = attributes.ok_or_else(windows::core::Error::empty)?;
        if let Some(device) = device {
            attributes.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, &device.manager)?;
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
        }
        // Converts the decoder's output to the BGRA asked for below.
        attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)?;
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), &attributes)?;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(VIDEO, true)?;
        let output = MFCreateMediaType()?;
        output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        // ARGB32 rather than RGB32: the video processor fills in alpha, and the UI draws with it.
        output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_ARGB32)?;
        let native = reader
            .GetNativeMediaType(VIDEO, 0)?
            .GetUINT64(&MF_MT_FRAME_SIZE)?;
        let (width, height) = ((native >> 32) as u32, native as u32);
        if let Some(max) = max_height.filter(|&max| max > 0 && max < height) {
            // Even sizes: the video processor rejects odd ones for some formats.
            let scaled = (u64::from(width) * u64::from(max) / u64::from(height)) as u32 & !1;
            output.SetUINT64(
                &MF_MT_FRAME_SIZE,
                (u64::from(scaled) << 32) | u64::from(max & !1),
            )?;
        }
        reader.SetCurrentMediaType(VIDEO, None, &output)?;
        Ok(reader)
    }
}

fn output_format(reader: &IMFSourceReader) -> windows::core::Result<(u32, u32, Duration)> {
    // SAFETY: reads properties of a configured reader; the duration PROPVARIANT is a VT_UI8.
    unsafe {
        let size = reader
            .GetCurrentMediaType(VIDEO)?
            .GetUINT64(&MF_MT_FRAME_SIZE)?;
        let duration = reader
            .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)?;
        let duration = duration.Anonymous.Anonymous.Anonymous.uhVal;
        Ok((
            (size >> 32) as u32,
            size as u32,
            Duration::from_nanos(duration.saturating_mul(100)),
        ))
    }
}

fn rewind(reader: &IMFSourceReader) -> windows::core::Result<()> {
    let mut start = PROPVARIANT::default();
    // SAFETY: a zeroed PROPVARIANT tagged VT_I8 is position zero, which is what
    // SetCurrentPosition expects with the null time format.
    unsafe {
        (*start.Anonymous.Anonymous).vt = VT_I8;
        reader.SetCurrentPosition(&GUID::zeroed(), &start)
    }
}

/// The software path: the frame is already in memory.
fn lock_and_copy(buffer: &IMF2DBuffer2, width: u32, height: u32) -> windows::core::Result<Vec<u8>> {
    let row = width as usize * 4;
    let mut pixels = Vec::with_capacity(row * height as usize);
    // SAFETY: the buffer is locked for reading, and rows are read within the reported pitch
    // before it is unlocked.
    unsafe {
        let mut scanline0 = std::ptr::null_mut();
        let mut pitch = 0i32;
        let mut start = std::ptr::null_mut();
        let mut len = 0u32;
        buffer.Lock2DSize(
            MF2DBuffer_LockFlags_Read,
            &mut scanline0,
            &mut pitch,
            &mut start,
            &mut len,
        )?;
        for y in 0..height as isize {
            let src = scanline0.offset(y * pitch as isize);
            pixels.extend_from_slice(std::slice::from_raw_parts(src, row));
        }
        buffer.Unlock2D()?;
    }
    // The software video processor can leave alpha at zero.
    if pixels.get(3) == Some(&0) {
        for px in pixels.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    }
    Ok(pixels)
}
