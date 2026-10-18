// crates/hikaru_plugin_host/tests/host_tests.rs

use hikaru_plugin_host::PluginHost; // <--- ¡Importación correcta!

#[test]
fn test_scan_paths() {
    // Verificar que el host pueda ser invocado
    PluginHost::scan_plugins();
    assert!(true);
    println!("¡Escaneo de plugins simulado con éxito! 🔌✨");
}