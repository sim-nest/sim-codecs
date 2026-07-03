# Run the comparison report

This recipe records how to measure the two wire codecs against each other: run
`cargo run --release -p sim-codec-compare --bin report` to print the per-category
size and speed tables that decide when bitwise is worth its cost.
