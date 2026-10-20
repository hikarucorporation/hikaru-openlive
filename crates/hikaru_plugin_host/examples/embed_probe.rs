// Sonda temporal: abre el editor de un plugin y lo deja vivo para poder
// inyectar clics y observar.

use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::Duration;

use hikaru_plugin_host::hikaru_wrapper::{HikaruWrapper, WrapperConfig, WrapperRuntime};
use hikaru_plugin_host::tap::NO_SLOT;
use hikaru_plugin_host::{HostParentKind, PluginFormat};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = PathBuf::from(
        args.get(1)
            .cloned()
            .unwrap_or_else(|| "/home/alex/.vst3/MonaChroma.vst3".to_string()),
    );
    let seconds: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);

    let (_cmd_tx, cmd_rx) = channel();
    let (feedback_tx, feedback_rx) =
        channel::<hikaru_plugin_host::hikaru_wrapper::ParamFeedback>();
    std::thread::spawn(move || {
        while let Ok(feedback) = feedback_rx.recv() {
            if !feedback.changes.is_empty() {
                println!("[probe] cambios: {:?}", feedback.changes);
            }
        }
    });

    let config = WrapperConfig::with_parent(HostParentKind::Wayland);
    let wrapper = match HikaruWrapper::open(
        &path,
        PluginFormat::VST3,
        "MonaChroma",
        config,
        WrapperRuntime::default(),
        cmd_rx,
        feedback_tx,
        NO_SLOT,
    ) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[probe] no se pudo abrir: {e}");
            std::process::exit(1);
        }
    };

    println!("[probe] abierto: {}", wrapper.plugin_name());
    let t = std::thread::spawn(move || {
        wrapper.run();
    });
    std::thread::sleep(Duration::from_secs(seconds));
    let _ = t.join();
}
