use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::fmt::Arguments;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    UI::{HiDpi::*, WindowsAndMessaging::*},
};

struct Trace {
    file: File,
    start: Instant,
}

pub fn event(name: &str, details: Arguments<'_>) {
    static TRACE: OnceLock<Mutex<Trace>> = OnceLock::new();
    let trace = TRACE.get_or_init(|| {
        let mut directory = std::path::PathBuf::from(
            std::env::var_os("LOCALAPPDATA").expect("DPI trace requires LOCALAPPDATA"),
        );
        directory.push("f0lock/diagnostics");
        std::fs::create_dir_all(&directory).expect("Cannot create DPI trace directory");
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = directory.join(format!("f0lock-{}-{stamp}.log", std::process::id()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("Cannot create DPI trace file");
        Mutex::new(Trace {
            file,
            start: Instant::now(),
        })
    });
    let mut trace = trace.lock().unwrap();
    let elapsed = trace.start.elapsed().as_secs_f64();
    writeln!(
        trace.file,
        "[DEBUG-f0dpi] {elapsed:.6} {:?} {name} {details}",
        std::thread::current().id()
    )
    .expect("Cannot write DPI trace");
}

fn native_window(hwnd: HWND) -> std::io::Result<String> {
    let mut client = RECT::default();
    let mut screen = RECT::default();
    unsafe {
        if GetClientRect(hwnd, &mut client) == 0 || GetWindowRect(hwnd, &mut screen) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(format!(
            "hwnd={hwnd:?} dpi={} awareness={} client={}x{} screen=({},{},{},{})",
            GetDpiForWindow(hwnd),
            GetAwarenessFromDpiAwarenessContext(GetWindowDpiAwarenessContext(hwnd)),
            client.right - client.left,
            client.bottom - client.top,
            screen.left,
            screen.top,
            screen.right,
            screen.bottom
        ))
    }
}

pub fn window_event(name: &str, window: &baseview::WindowContext, details: Arguments<'_>) {
    let handle = window.platform_handle();
    let RawWindowHandle::Win32(handle) = handle.window_handle().unwrap().as_raw() else {
        unreachable!("DPI trace expects a Windows HWND");
    };
    let hwnd = handle.hwnd.get() as HWND;
    let parent = unsafe { GetParent(hwnd) };
    let own = native_window(hwnd);
    let parent = (!parent.is_null()).then(|| native_window(parent));
    let awareness = unsafe { GetAwarenessFromDpiAwarenessContext(GetThreadDpiAwarenessContext()) };
    event(
        name,
        format_args!(
            "thread_awareness={awareness} baseview={:?} own={own:?} parent={parent:?} {details}",
            window.size()
        ),
    );
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GpuCounters {
    pub configurations: u64,
    pub frames: u64,
}

pub struct GpuTiming {
    pub counters: GpuCounters,
    pub ui_ms: f64,
    epoch: Instant,
    last_frame: Instant,
    samples: u64,
    totals: [f64; 6],
    maxima: [f64; 6],
}

impl Default for GpuTiming {
    fn default() -> Self {
        Self {
            counters: GpuCounters::default(),
            ui_ms: 0.0,
            epoch: Instant::now(),
            last_frame: Instant::now(),
            samples: 0,
            totals: [0.0; 6],
            maxima: [0.0; 6],
        }
    }
}

impl GpuTiming {
    pub fn frame(
        &mut self,
        window: &baseview::WindowContext,
        stages: [f64; 4],
        ctx: &egui::Context,
    ) {
        let now = Instant::now();
        let values = [
            self.ui_ms,
            stages[0],
            stages[1],
            stages[2],
            stages[3],
            now.duration_since(self.last_frame).as_secs_f64() * 1000.0,
        ];
        self.last_frame = now;
        self.samples += 1;
        self.counters.frames += 1;

        for ((total, max), value) in self.totals.iter_mut().zip(&mut self.maxima).zip(values) {
            *total += value;
            *max = max.max(value);
        }

        if self.counters.frames == 1 || self.epoch.elapsed() >= Duration::from_secs(1) {
            let mean = self.totals.map(|sum| sum / self.samples as f64);
            window_event(
                "frame_stats",
                window,
                format_args!(
                    "zoom={} pixels_per_point={} native_ppp={:?} content={:?} frames={} configs={} mean_ms[ui,render,acquire,submit,present,gap]={mean:?} max_ms={:?}",
                    ctx.zoom_factor(),
                    ctx.pixels_per_point(),
                    ctx.native_pixels_per_point(),
                    ctx.content_rect(),
                    self.counters.frames,
                    self.counters.configurations,
                    self.maxima
                ),
            );
            self.epoch = now;
            self.samples = 0;
            self.totals = [0.0; 6];
            self.maxima = [0.0; 6];
        }
    }
}
