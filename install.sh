echo 'version = 0.1' > src/templates.toml
cat templates/*.toml >> src/templates.toml
cargo build
cp ~/bin/chomp ~/bin/chomp2
cp ./target/Debug/chomp ~/bin/chomp
