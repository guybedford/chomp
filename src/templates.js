
Chomp.registerTemplate('babel', function ({ name, targets, deps, env, templateOptions: { presets = [], plugins = [], sourceMap = true, noBabelRc = false, configFile = null, autoInstall, ...invalid } }, { CHOMP_EJECT }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid babel template option "${Object.keys(invalid)[0]}"`);
  const defaultConfig = {};
  return [{
    name,
    targets,
    deps: [...deps, ...noBabelRc || CHOMP_EJECT ? [] : ['.babelrc'], ...CHOMP_EJECT ? [] : presets.map(p => `node_modules/${p}`), ...plugins.map(p => `node_modules/${p}`), ...CHOMP_EJECT ? [] : ['node_modules/@babel/core', 'node_modules/@babel/cli']],
    env,
    run: `babel $DEP -o $TARGET${
        sourceMap ? ' --source-maps' : ''
      }${
        plugins.length ? ` --plugins=${plugins.join(',')}` : ''
      }${
        presets.length ? ` --presets=${presets.join(',')}` : ''
      }${
        noBabelRc ? ' --no-babelrc' : ''
      }${
        configFile ? ` --config-file=${configFile.startsWith('./') ? configFile : './' + configFile}` : ''
      }`
  }, ...CHOMP_EJECT ? [] : [{
    target: '.babelrc',
    display: false,
    invalidation: 'not-found',
    run: `
      echo '\n\x1b[93mChomp\x1b[0m: Creating \x1b[1m.babelrc\x1b[0m (set \x1b[1m"no-babel-rc = true"\x1b[0m Babel template option to skip)\n'
      echo '${JSON.stringify(defaultConfig, null, 2)}' > .babelrc
    `
  }, {
    template: 'npm',
    templateOptions: {
      packages: [...presets.map(p => p.startsWith('@babel/') ? p + '@7' : p), ...plugins.map(p => p.startsWith('@babel/') ? p + '@7' : p), '@babel/core@7', '@babel/cli@7'],
      dev: true,
      autoInstall
    }
  }]];
});

Chomp.registerBatcher('babel', function (batch, _running) {
  const run_completions = {};
  let existingBabelRcInit = null;
  for (const { id, run, engine } of batch) {
    if (engine !== 'cmd' || !run.trimLeft().startsWith('echo ')) continue;
    if (run.indexOf('Creating \x1b[1m.babelrc\x1b[0m') !== -1) {
      if (existingBabelRcInit !== null) {
        run_completions[id] = existingBabelRcInit;  
      }
      else {
        existingBabelRcInit = id;
      }
      continue;
    }
  }
  return [[], [], run_completions];
});
Chomp.registerTemplate('cargo', function ({ deps, env, templateOptions: { bin, install, ...invalid } }, { PATH, CHOMP_EJECT }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid cargo template option "${Object.keys(invalid)[0]}"`);
  const sep = PATH.match(/\\|\//)[0];
  return CHOMP_EJECT ? [] : [{
    name: `cargo:${bin}`,
    targets: [PATH.split(';').find(p => p.endsWith(`.cargo${sep}bin`)) + sep + bin + (sep === '/' ? '' : '.exe')],
    invalidation: 'not-found',
    display: false,
    deps,
    env,
    run: `cargo install ${install}`
  }];
});
Chomp.registerTemplate('jspm', function ({ name, targets, deps, env, templateOptions: {
  autoInstall,
  env: generatorEnv = ['browser', 'production', 'module'],
  preload,
  integrity,
  whitespace,
  esModuleShims,
  ...generateOpts
} }, { CHOMP_EJECT }) {
  const mainTarget = targets.find(target => target.includes('#')) || targets[0];
  const isImportMapTarget = mainTarget && mainTarget.endsWith('.importmap');
  const { resolutions } = generateOpts;
  const noHtmlOpts = preload === undefined && integrity === undefined && whitespace === undefined && esModuleShims === undefined;
  return [{
    name,
    targets,
    invalidation: 'always',
    deps: [...deps, ...CHOMP_EJECT ? [] : ['node_modules/@jspm/generator', 'node_modules/mkdirp']],
    env,
    engine: 'node',
    run: `    import { Generator } from '@jspm/generator';
    import { readFile, writeFile } from 'fs/promises';
    import { pathToFileURL } from 'url';
    import mkdirp from 'mkdirp';
    import { dirname } from 'path';

    const generator = new Generator({
      mapUrl: ${isImportMapTarget ? 'import.meta.url' : 'pathToFileURL(process.env.TARGET)'}${
        resolutions && !isImportMapTarget && Object.values(resolutions).some(v => v.startsWith('./') || v.startsWith('../')) ? ',\n      baseUrl: new URL(\'.\', import.meta.url)' : ''
      },\n      env: ${JSON.stringify(generatorEnv).replace(/","/g, '", "')}${
        Object.keys(generateOpts).length ? ',\n      ' + JSON.stringify(generateOpts, null, 2).slice(4, -2).replace(/\n/g, `\n    `) : ''
      }
    });
${isImportMapTarget ? `
    await Promise.all(process.env.DEPS.split(',')${CHOMP_EJECT ? '' : '.filter(dep => dep !== "node_modules/@jspm/generator" && dep !== "node_modules/mkdirp")'}.map(dep => generator.traceInstall('./' + dep)));

    mkdirp.sync(dirname(process.env.TARGET));
    await writeFile(process.env.TARGET, JSON.stringify(generator.getMap(), null, 2));`
: `
    const htmlSource = await readFile(process.env.DEP, 'utf-8');

    mkdirp.sync(dirname(process.env.TARGET));
    await writeFile(process.env.TARGET, await generator.htmlGenerate(htmlSource, {
      htmlUrl: pathToFileURL(process.env.TARGET)${noHtmlOpts ? '' : ',      ' + JSON.stringify({ preload, integrity, whitespace, esModuleShims })}
    }));`}
`
  }, ...CHOMP_EJECT ? [] : [{
    template: 'npm',
    templateOptions: {
      autoInstall,
      packages: ['@jspm/generator', 'mkdirp'],
      dev: true
    }
  }]];
});
Chomp.registerTemplate('npm', function ({ name, deps, env, templateOptions: { packages, dev, packageManager = 'npm', autoInstall, ...invalid } }, { CHOMP_EJECT }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid npm template option "${Object.keys(invalid)[0]}"`);
  if (!packages)
    throw new Error('npm template requires the "packages" option to be a list of packages to install.');
  return CHOMP_EJECT ? [] : autoInstall ? [{
    name,
    deps: [...deps, ...packages.map(pkg => {
      const versionIndex = pkg.indexOf('@', 1);
      return `node_modules/${versionIndex === -1 ? pkg : pkg.slice(0, versionIndex)}`;
    })],
    serial: true
  }, ...packages.map(pkg => {
    const versionIndex = pkg.indexOf('@', 1);
    return {
      target: `node_modules/${versionIndex === -1 ? pkg : pkg.slice(0, versionIndex)}`,
      invalidation: 'not-found',
      display: false,
      deps: ['npm:init'],
      env,
      run: `${packageManager} install ${packages.join(' ')}${dev ? ' -D' : ''}`
    };
  }), {
    name: 'npm:init',
    target: 'package.json',
    invalidation: 'not-found',
    display: false,
    env,
    run: `${packageManager} init -y`
  }] : [{
    name,
    env,
    invalidation: 'not-found',
    display: false,
    targets: packages.map(pkg => {
      const versionIndex = pkg.indexOf('@', 1);
      return `node_modules/${versionIndex === -1 ? pkg : pkg.slice(0, versionIndex)}`;
    }),
    run: `echo "\n\x1b[93mChomp\x1b[0m: Some packages are missing. Please run \x1b[1m${packageManager} install ${packages.join(' ')}${dev ? ' -D' : ''}\x1b[0m\n"`
  }];
});

// Batcher for npm executions handles the following:
// 1. Ensuring only one npm operation runs at a time
// 2. If two npm init operations are batched, only one is run. If npm init
//    is already running, ties additional invocations to the existing one.
// 3. When multiple npm install operations are running at the same time,
//    combine them into a single install operation.
Chomp.registerBatcher('npm', function (batch, running) {
  const queued = [], run_completions = {};
  let batchInstall = null;
  for (const { id, run, engine, env } of batch) {
    if (engine !== 'cmd' || !run.startsWith('npm ')) continue;
    const args = run.slice(4).split(' ');
    if (args[0] === 'init' && args[1] === '-y' && args.length === 2) {
      const existingNpm = running.find(({ run }) => run.startsWith('npm '));
      if (existingNpm) {
        run_completions[id] = existingNpm.id;
        continue;
      }
    }
    if (args[0] === 'install') {
      const install = parseInstall(args.slice(1));
      if (!install) return;
      if (running.find(({ run }) => run.startsWith('npm ')) ||
          batchInstall && batchInstall.isDev !== install.isDev) {
        queued.push(id);
        continue;
      }
      if (!batchInstall) {
        batchInstall = { isDev: install.isDev, env, engine, ids: [id], packages: install.packages };
      }
      else {
        for (const key of Object.keys(env)) {
          if (!Object.hasOwnProperty.call(batchInstall.env, key))
            batchInstall.env[key] = env[key];
        }
        batchInstall.ids.push(id);
        for (const pkg of install.packages) {
          if (!batchInstall.packages.includes(pkg))
            batchInstall.packages.push(pkg);
        }
      }
    }
  }
  const run = batchInstall ? [{
    run: `npm install${batchInstall.isDev ? ' -D' : ''} ${batchInstall.packages.join(' ')}`,
    env: batchInstall.env,
    engine: batchInstall.engine,
    ids: batchInstall.ids,
  }] : [];
  return [queued, run, run_completions];

  function parseInstall (args) {
    const packages = args.filter(arg => !arg.startsWith('-') && arg.indexOf('"') === -1 && arg.indexOf("'") === -1);
    const flags = args.filter(arg => arg.startsWith('-'));
    if (flags.length > 1) return;
    if (flags.length === 1 && flags[0] !== '-D') return;
    if (packages.length + flags.length !== args.length) return;
    const isDev = flags.length === 1;
    return { packages, isDev };
  }
});
Chomp.registerTemplate('prettier', function ({ name, targets, deps, env, templateOptions: { files = '.', check = false, write = true, config = null, noErrorOnUnmatchedPattern = false, autoInstall, ...invalid } }, { CHOMP_EJECT }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid prettier template option "${Object.keys(invalid)[0]}"`);
  return [{
    name,
    targets,
    deps: [...deps, ...CHOMP_EJECT ? [] : ['node_modules/prettier']],
    invalidation: 'always',
    env,
    run: `prettier ${files} ${
        check ? ' --check' : ''
      }${
        write ? ' --write' : ''
      }${
        config ? ` --config ${config}` : ''
      }${
        noErrorOnUnmatchedPattern ? ' --no-error-on-unmatched-pattern' : ''
      }`
  }, ...CHOMP_EJECT ? [] : [{
    template: 'npm',
    templateOptions: {
      autoInstall,
      packages: ['prettier'],
      dev: true
    }
  }]];
});
Chomp.registerTemplate('svelte', function ({ name, targets, deps, env, templateOptions: { svelteConfig = null, autoInstall, ...invalid } }, { CHOMP_EJECT }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid svelte template option "${Object.keys(invalid)[0]}"`);
  return [{
    name,
    targets,
    deps: [...deps, ...CHOMP_EJECT ? [] : ['node_modules/svelte', 'node_modules/mkdirp']],
    env,
    engine: 'node',
    run: `    import { readFile, writeFile } from 'fs/promises';
      import { compile } from 'svelte/compiler';
      import mkdirp from 'mkdirp';
      import { dirname } from 'path';

      let config;
      ${svelteConfig ? `
        config = await import(${svelteConfig === true ? '"./svelte.config.js"' : svelteConfig});
      ` : `
        config = {
          css: false
        };
      `}
      config.filename = process.env.DEP;

      const source = await readFile(process.env.DEP, 'utf-8');
      const result = compile(source, config);

      mkdirp.sync(dirname(process.env.TARGET));
      const cssFile = process.env.TARGET.replace(/\\.js$/, ".css");
      await Promise.all[
        writeFile(process.env.TARGET, result.js.code),
        writeFile(process.env.TARGET + ".map", JSON.stringify(result.js.map)),
        writeFile(cssFile, result.css.code),
        writeFile(cssFile + ".map", JSON.stringify(result.css.map))
      ];
    `
  }, ...CHOMP_EJECT ? [] : [{
    template: 'npm',
    templateOptions: {
      autoInstall,
      packages: ['svelte@3', 'mkdirp'],
      dev: true
    }
  }]];
});
Chomp.registerTemplate('swc', function ({ name, targets, deps, env, templateOptions: { configFile = null, noSwcRc = false, sourceMaps = true, config = {}, autoInstall, ...invalid } }, { PATH, CHOMP_EJECT }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid swc template option "${Object.keys(invalid)[0]}"`);
  const isWin = PATH.match(/\\|\//)[0] !== '/';
  const defaultConfig = {
    jsc: {
      parser: {
        syntax: 'typescript',
        importAssertions: true,
        topLevelAwait: true,
        importMeta: true,
        privateMethod: true,
        dynamicImport: true
      }/*,
      experimental: {
        keepImportAssertions: true
      }*/ // TODO: reenable when supported
    }
  };
  function setDefaultConfig (config, defaultConfig, base = '') {
    for (const prop of Object.keys(defaultConfig)) {
      const val = defaultConfig[prop];
      if (typeof val === 'object') {
        setDefaultConfig(config, defaultConfig[prop], base + prop + '.');
      }
      else if (!((base + prop) in config)) {
        config[base + prop] = defaultConfig[prop];
      }
    }
  }
  if (noSwcRc) {
    setDefaultConfig(config, defaultConfig);
  }
  return [{
    name,
    targets,
    deps: [...deps, ...noSwcRc || CHOMP_EJECT ? [] : ['.swcrc'], ...CHOMP_EJECT ? [] : ['node_modules/@swc/core', 'node_modules/@swc/cli']],
    env,
    run: `node ./node_modules/@swc/cli/bin/swc.js $DEP -o $TARGET${
        noSwcRc ? ' --no-swcrc' : ''
      }${
        configFile ? ` --config-file=${configFile}` : ''
      }${
        sourceMaps ? ' --source-maps' : ''
      }${
        Object.keys(config).length ? ' ' + Object.keys(config).map(key => `-C ${key}=${config[key]}`).join(' ') : ''
      }`
  }, ...CHOMP_EJECT ? [] : [{
    target: '.swcrc',
    invalidation: 'not-found',
    display: false,
    run: `
      echo '\n\x1b[93mChomp\x1b[0m: Creating \x1b[1m.swcrc\x1b[0m (set \x1b[1m"no-swc-rc = true"\x1b[0m SWC template option to skip)\n'
      ${isWin // SWC does not like a BOM... Powershell hacks...
        ? `$encoder = new-object System.Text.UTF8Encoding ; Set-Content -Value $encoder.Getbytes('${JSON.stringify(defaultConfig, null, 2)}') -Encoding Byte -Path $TARGET`
        : `echo '${JSON.stringify(defaultConfig)}' > $TARGET`
      }
    `
  }, {
    template: 'npm',
    templateOptions: {
      autoInstall,
      packages: ['@swc/core@1', '@swc/cli@0.1'],
      dev: true
    }
  }]];
});

// Batcher to ensure swcrc log only appears once
Chomp.registerBatcher('swc', function (batch, running) {
  const run_completions = {};
  let existingSwcRcInit = null;
  for (const { id, run, engine, env } of batch) {
    if (engine !== 'cmd' || !run.trimLeft().startsWith('echo ')) continue;
    if (run.indexOf('Creating \x1b[1m.swcrc\x1b[0m') !== -1) {
      if (existingSwcRcInit !== null) {
        run_completions[id] = existingSwcRcInit;
      }
      else {
        existingSwcRcInit = id;
      }
      continue;
    }
  }
  return [[], [], run_completions];
});
