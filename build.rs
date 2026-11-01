use std::path::Path;

/// Writes `engine_pack.rs` in `OUT_DIR`: the engine's content pack's files,
/// those `assets/pack.txt` lists, each by its path in the pack with its
/// bytes built in (`include_bytes!`), for `src/content.rs` to serve from
/// memory in a release build and on the web.
fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    println!("cargo:rerun-if-changed=assets");
    let list = std::fs::read_to_string(root.join("pack.txt")).expect("assets/pack.txt");
    let mut code = String::from("pub const FILES: &[(&str, &[u8])] = &[\n");
    for file in list
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let path = root.join(file);
        assert!(path.is_file(), "assets/pack.txt: no file assets/{file}");
        code += &format!(
            "    ({file:?}, include_bytes!({:?})),\n",
            path.display().to_string()
        );
    }
    code += "];\n";
    let out = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("engine_pack.rs");
    std::fs::write(out, code).expect("engine_pack.rs written");
}
