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
