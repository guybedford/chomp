$PSDefaultParameterValues['Out-File:Encoding'] = 'utf8';
echo 'version = 0.1' > src/templates.toml
cat templates/*.toml >> src/templates.toml
cargo build
cp ~/bin/chomp.exe ~/bin/chomp2.exe
cp ./target/Debug/chomp.exe ~/bin/chomp.exe
