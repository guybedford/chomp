#!/usr/bin/env node

import BinWrapper from 'bin-wrapper';
import { readFileSync } from 'fs';
import path from 'path';
import { spawn } from 'child_process';
import { fileURLToPath } from 'url';

let { version } = JSON.parse(readFileSync(new URL('package.json', import.meta.url), 'utf8'));
if (version.match(/-rebuild(\.\d)?$/))
  version = version.split('-rebuild')[0];
const base = `https://github.com/guybedford/chomp/releases/download/${version}`

const bin = new BinWrapper({ skipCheck: true })
  .src(`${base}/chomp-macos-${version}.tar.gz`, 'darwin')
  .src(`${base}/chomp-linux-${version}.tar.gz`, 'linux', 'x64')
  .src(`${base}/chomp-windows-${version}.zip`, 'win32', 'x64')
  .dest(path.join('vendor'))
  .use(process.platform === 'win32' ? 'chomp.exe' : 'chomp')
  .version(version);

await bin.run();

spawn(fileURLToPath(new URL(bin.path(), import.meta.url)), process.argv.slice(2), { stdio: 'inherit' });
