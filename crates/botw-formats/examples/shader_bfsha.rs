//! Inspect Wii U BFSHA 4.5.0.4 metadata and extract native VS/PS bytecode.
//! `cargo run --offline -p botw-formats --example shader_bfsha -- FILE [--entry NAME] [--out DIRECTORY]`
use botw_formats::{shader_bfsha::BfshaArchive, yaz0::decompress_if};
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
            "--entry" => entries.push(args.next().ok_or("missing entry")?),
            "--out" => {
                out = Some(PathBuf::from(
                    args.next().ok_or("missing output directory")?,
                ))
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    let mut bytes = decompress_if(&std::fs::read(&path)?)?.into_owned();
    for entry in &entries {
        let sarc = roead::sarc::Sarc::new(&bytes[..])?;
        bytes = decompress_if(sarc.get_data(entry).ok_or("missing SARC entry")?)?.into_owned();
    }
    let archive = BfshaArchive::parse(&bytes)?;
    if let Some(out) = &out {
        std::fs::create_dir_all(out)?;
        std::fs::write(out.join("archive.bfsha"), &bytes)?;
    }
    let mut report = format!(
        "source={path}\nentries={entries:?}\narchive={} version=0x04050004\n",
        archive.name
    );
    for (mi, model) in archive.models.iter().enumerate() {
        writeln!(
            report,
            "model {mi} {:?} offset={:#x} programs={} static_options={} dynamic_options={}",
            model.name,
            model.offset,
            model.programs.len(),
            model.static_options.len(),
            model.dynamic_options.len()
        )?;
        for option in model.static_options.iter().chain(&model.dynamic_options) {
            writeln!(
                report,
                "  option {} word={} shift={} mask={:#x} default={} choices={:?}",
                option.name,
                option.word_index,
                option.shift,
                option.mask,
                option.default_choice,
                option.choices
            )?;
        }
        for (kind, records) in [
            ("attribute", &model.attributes),
            ("sampler", &model.samplers),
        ] {
            for record in records {
                writeln!(
                    report,
                    "  {kind} {} @{:#x} {:02x?}",
                    record.name, record.offset, record.data
                )?;
            }
        }
        for block in &model.uniform_blocks {
            writeln!(
                report,
                "  block {} @{:#x} {:02x?}",
                block.record.name, block.record.offset, block.record.data
            )?;
            for uniform in &block.uniforms {
                writeln!(
                    report,
                    "    uniform {} @{:#x} {:02x?}",
                    uniform.name, uniform.offset, uniform.data
                )?;
            }
        }
        for (pi, program) in model.programs.iter().enumerate() {
            writeln!(
                report,
                "  program {pi} @{:#x} flags={:#x} keys={:08x?}",
                program.offset, program.flags, program.key
            )?;
            for option in model.static_options.iter().chain(&model.dynamic_options) {
                writeln!(
                    report,
                    "    {}={}",
                    option.name,
                    option.choice(&program.key)?
                )?;
            }
            writeln!(
                report,
                "    sampler_locations={:02x?}\n    block_locations={:02x?}",
                program.sampler_locations, program.block_locations
            )?;
            for (stage, code) in [("vs", &program.vertex), ("ps", &program.pixel)] {
                if let Some(code) = code {
                    writeln!(
                        report,
                        "    {stage} header={:#x} code={:#x?}",
                        code.header_offset, code.range
                    )?;
                    if let Some(out) = &out {
                        std::fs::write(
                            out.join(format!("model{mi:03}-program{pi:05}-{stage}.code")),
                            code.bytes,
                        )?;
                    }
                }
            }
            for stage in [1, 2, 4] {
                if let Some(offset) = program.stage_offsets[stage] {
                    writeln!(
                        report,
                        "    unsupported stage {stage} @{offset:#x}; code not decoded"
                    )?;
                }
            }
        }
    }
    if let Some(out) = out {
        std::fs::write(out.join("manifest.txt"), report)?;
        println!(
            "{}: {} models, {} programs; {}",
            archive.name,
            archive.models.len(),
            archive
                .models
                .iter()
                .map(|m| m.programs.len())
                .sum::<usize>(),
            out.display()
        );
    } else {
        print!("{report}");
    }
    Ok(())
}
