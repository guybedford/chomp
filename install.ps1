$PSDefaultParameterValues['Out-File:Encoding'] = 'utf8';
echo '' > src/templates.js
cat templates/*.js >> src/templates.js
cargo build
cp ~/bin/chomp.exe ~/bin/chomp2.exe
cp ./target/Debug/chomp.exe ~/bin/chomp.exe
