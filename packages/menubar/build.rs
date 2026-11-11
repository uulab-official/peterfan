use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let directory = manifest.join("../../assets/runner/color");
    // Watch the deepest existing parent: Cargo otherwise reruns every time for
    // a nonexistent path. As directories are created the next build watches
    // one level deeper, and existing files are also watched individually.
    let mut watched = directory.as_path();
    while !watched.exists() {
        watched = watched.parent().expect("manifest directory exists");
    }
    println!("cargo:rerun-if-changed={}", watched.display());
    let names = [
        "cat", "dog", "rabbit", "fox", "penguin", "dinosaur", "robot", "ghost", "bird", "duck",
        "sheep", "fish", "slime", "human", "pushup", "orbit", "sushi", "city", "sausage", "party",
    ];
    let mut registry =
        String::from("fn embedded_strip(name: &str) -> Option<&'static [u8]> {\nmatch name {\n");
    for name in names {
        let path = directory.join(format!("{name}.png"));
        if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
            registry.push_str(&format!(
                "{name:?} => Some(include_bytes!({:?})),\n",
                path.to_string_lossy()
            ));
        }
    }
    registry.push_str("_ => None,\n}\n}\n");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("build output directory"));
    fs::write(out.join("color_runner_registry.rs"), registry).expect("write color runner registry");
}
