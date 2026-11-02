fn main() {
    // The build number shown in the app (Settings > About and the window title).
    // GitHub jobs set MDP_BUILD, MDP_COMMIT and MDP_BUILD_DATE; local builds say "local build".
    for v in ["MDP_BUILD", "MDP_COMMIT", "MDP_BUILD_DATE"] {
        println!("cargo:rerun-if-env-changed={v}");
    }
    tauri_build::build()
}
