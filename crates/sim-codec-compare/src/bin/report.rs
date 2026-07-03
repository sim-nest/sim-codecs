//! Print the bitwise-vs-binary size and speed comparison tables.

fn main() {
    println!("# sim-codec-compare: bitwise vs binary\n");
    println!("## Size (bytes, mean per category; ratio < 1.0 = bitwise smaller)\n");
    println!("{}", sim_codec_compare::report::size_table());
    println!("\n## Speed (bitwise/binary slowdown; > 1.0 = bitwise slower)\n");
    println!("{}", sim_codec_compare::report::speed_table(200));
}
