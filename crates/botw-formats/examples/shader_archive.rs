//! Inspect a decompressed/Yaz0 SHARCB or an entry in a SARC; optionally extract
//! original GX2 payloads and code outside the repository.
//! `cargo run --offline -p botw-formats --example shader_archive -- FILE [--entry NAME] [--out DIRECTORY]`

use botw_formats::{shader::ShaderArchive, yaz0::decompress_if};
use std::{fmt::Write, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("expected FILE [--entry NAME] [--out DIRECTORY]")?;
    let mut entries = Vec::new();
    let mut out = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--entry" => entries.push(args.next().ok_or("--entry requires NAME")?),
            "--out" => {
                out = Some(PathBuf::from(
                    args.next().ok_or("--out requires DIRECTORY")?,
                ))
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    let mut bytes = decompress_if(&std::fs::read(&path)?)?.into_owned();
    for entry in &entries {
        let archive = roead::sarc::Sarc::new(&bytes[..])?;
        bytes =
            decompress_if(archive.get_data(entry).ok_or("entry missing in SARC")?)?.into_owned();
    }
    let archive = ShaderArchive::parse(&bytes)?;
    let mut report = format!(
        "source={path}\nentries={entries:?}\narchive={} version=9 flags=5 binaries={} programs={}\n",
        archive.name,
        archive.binaries.len(),
        archive.programs.len()
    );
    for program in &archive.programs {
        writeln!(
            report,
            "program {:?} record={:#x?} kind={} base={} variations={}",
            program.name,
            program.record_range,
            program.kind,
            program.base_index,
            program.variation_count
        )?;
        for var in &program.variations {
            writeln!(
                report,
                "  macro {}={:?} symbol={:?}",
                var.name, var.values, var.symbol
            )?;
        }
        let stride = match program.kind {
            3 => 2,
            7 => 3,
            8 => 1,
            _ => unreachable!(),
        };
        for variant in 0..program.variation_count {
            let mut index = variant;
            let mut choices = Vec::new();
            for var in program.variations.iter().rev() {
                choices.push(format!(
                    "{}={}",
                    var.name,
                    var.values[index % var.values.len()]
                ));
                index /= var.values.len();
            }
            choices.reverse();
            writeln!(report, "  variant {variant}: {}", choices.join(", "))?;
            for stage in 0..stride {
                let index = program.base_index + variant * stride + stage;
                let binary = &archive.binaries[index];
                writeln!(
                    report,
                    "    binary {index} stage={} record={:#x?} unknown={:#x}",
                    binary.stage, binary.record_range, binary.unknown_word
                )?;
                if let Some(shader) = &binary.gx2 {
                    writeln!(
                        report,
                        "      code={:#x?} bytes={}",
                        shader.code_range,
                        shader.code.len()
                    )?;
                    for (label, bindings) in [
                        ("block", &shader.uniform_blocks),
                        ("uniform", &shader.uniforms),
                        ("sampler", &shader.samplers),
                        ("attribute", &shader.attributes),
                    ] {
                        for binding in bindings {
                            writeln!(report, "      {label} {} {:?}", binding.name, binding.words)?;
                        }
                    }
                } else {
                    writeln!(
                        report,
                        "      unsupported GX2 stage: payload preserved, code not decoded"
                    )?;
                }
            }
        }
    }
    if let Some(out) = out {
        std::fs::create_dir_all(&out)?;
        std::fs::write(out.join("archive.sharcb"), &bytes)?;
        std::fs::write(out.join("manifest.txt"), &report)?;
        for (index, binary) in archive.binaries.iter().enumerate() {
            std::fs::write(
                out.join(format!("{index:05}-stage{}.gx2", binary.stage)),
                binary.data,
            )?;
            if let Some(shader) = &binary.gx2 {
                std::fs::write(
                    out.join(format!("{index:05}-stage{}.code", binary.stage)),
                    shader.code,
                )?;
            }
        }
        println!(
            "{}: {} programs, {} binaries; {}",
            archive.name,
            archive.programs.len(),
            archive.binaries.len(),
            out.display()
        );
    } else {
        print!("{report}");
    }
    Ok(())
}
