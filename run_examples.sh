# Run examples - this also generates .profraw files
echo "Running examples..."
for example_file in examples/*.rs; do
    if [ -f "$example_file" ]; then
        example_name=$(basename "$example_file" .rs)
        echo "Running example: $example_name"
        cargo run --example "$example_name"
    fi
done

