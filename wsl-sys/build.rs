#[cfg(not(target_os = "windows"))]
const _: () = assert!(false, "TTD bindings only work on Windows");

#[cfg(feature = "embed-runtime")]
use std::path::Path;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=sdk");

    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let winmd = out.join("Microsoft.WSL.Containers.winmd");
    windows_rdl::reader()
        .input("sdk/Microsoft.WSL.Containers.rdl")
        .reference_default()
        .output(&winmd)
        .write()
        .unwrap_or_else(|e| panic!("failed to compile Microsoft.WSL.Containers.rdl: {e}"));

    windows_bindgen::bindgen([
        "--in",
        "default",
        winmd.to_str().unwrap(),
        "--out",
        out.join("bindings.rs").to_str().unwrap(),
        "--filter",
        "Microsoft.WSL.Containers",
        "Windows.Storage.Streams.IInputStream",
        "Windows.Storage.Streams.IOutputStream",
        "Windows.Storage.Streams.DataReader",
        "Windows.Storage.Streams.DataWriter",
        "Windows.Storage.Streams.DataWriterStoreOperation",
        "Windows.Storage.Streams.InputStreamOptions",
        "Windows.Storage.Streams.Buffer",
        "Windows.Storage.Streams.IBuffer",
    ]);

    #[cfg(feature = "embed-runtime")]
    embed_runtime(&out, &winmd);
}

#[cfg(feature = "embed-runtime")]
fn embed_runtime(out: &Path, winmd: &Path) {
    let dll = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "x86_64" => "sdk/x64/wslcsdk.dll",
        "aarch64" => "sdk/arm64/wslcsdk.dll",
        arch => panic!("wslcsdk.dll is only available for x86_64 and aarch64, not {arch}"),
    };
    let dll = std::fs::canonicalize(dll).unwrap();
    let manifest_path = out.join("wslcsdk.manifest");
    std::fs::write(&manifest_path, manifest(winmd)).unwrap();

    let rc = out.join("wslcsdk.rc");
    std::fs::write(
        &rc,
        format!(
            "101 RCDATA \"{}\"\n102 RCDATA \"{}\"\n",
            to_rc_path(&dll),
            to_rc_path(&manifest_path)
        ),
    )
    .unwrap();
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_required()
        .unwrap();
}

#[cfg(feature = "embed-runtime")]
fn to_rc_path(path: &Path) -> String {
    let path = path.to_str().unwrap();
    path.strip_prefix(r"\\?\").unwrap_or(path).replace('\\', r"\\")
}

#[cfg(feature = "embed-runtime")]
fn manifest(winmd: &Path) -> String {
    use windows_metadata::HasAttributes;
    use windows_metadata::reader::{Index, TypeCategory};

    let index = Index::read(winmd).expect("invalid winmd");
    let mut classes: Vec<String> = index
        .types()
        .filter(|ty| ty.namespace() == "Microsoft.WSL.Containers")
        .filter(|ty| ty.category() == TypeCategory::Class)
        .filter(|ty| ty.has_attribute("ActivatableAttribute") || ty.has_attribute("StaticAttribute"))
        .map(|ty| format!("{}.{}", ty.namespace(), ty.name()))
        .collect();
    classes.sort();
    assert!(!classes.is_empty(), "no activatable classes found in the winmd");

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <assembly xmlns=\"urn:schemas-microsoft-com:asm.v1\" manifestVersion=\"1.0\">\n  \
         <file name=\"wslcsdk.dll\">\n",
    );
    for class in classes {
        xml += &format!(
            "    <activatableClass name=\"{class}\" threadingModel=\"both\" xmlns=\"urn:schemas-microsoft-com:winrt.v1\" />\n"
        );
    }
    xml += "  </file>\n</assembly>\n";
    xml
}
