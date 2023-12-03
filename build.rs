use std::process::Command;

fn main() {
    let output_dir = std::env::var("OUT_DIR").expect("No output directory");
    let output_file = format!("{}/sample_program.elf", output_dir);
    Command::new("i686-elf-gcc")
        .arg("-fPIC")
        .arg("-nostdlib")
        .arg("-o")
        .arg(output_file)
        .arg("res/sample_program/sample.c")
        .status()
        .expect("Failed to run build for sample program");

    println!("cargo:rerun-if-changed=res/sample_program");
}
