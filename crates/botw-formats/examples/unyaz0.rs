//! Decompresses a Yaz0 file: `cargo run -p botw-formats --example unyaz0 -- <in> <out>`
fn main() {
    let mut args = std::env::args().skip(1);
    let (input, output) = (args.next().expect("input"), args.next().expect("output"));
    let data = botw_formats::yaz0::decompress_if(&std::fs::read(input).unwrap()).unwrap().into_owned();
    std::fs::write(output, data).unwrap();
}
