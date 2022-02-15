Chomp.addExtension('./npm.js');

const defaultConfig = {};

Chomp.registerTask({
  target: '.babelrc',
  display: false,
  invalidation: 'not-found',
  run: `
    echo '\n\x1b[93mChomp\x1b[0m: Creating \x1b[1m.babelrc\x1b[0m (\x1b[1m"babel-rc = true"\x1b[0m Babel template option in use)\n'
    echo '${JSON.stringify(defaultConfig, null, 2)}' > .babelrc
  `
});

Chomp.registerTemplate('babel', function ({ name, targets, deps, env, templateOptions: { presets = [], plugins = [], sourceMap = true, babelRc = false, configFile = null, autoInstall, ...invalid } }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid babel template option "${Object.keys(invalid)[0]}"`);
  return [{
    name,
    targets,
    deps: [...deps, ...!babelRc || ENV.CHOMP_EJECT ? [] : ['.babelrc'], ...ENV.CHOMP_EJECT ? [] : presets.map(p => `node_modules/${p}`), ...plugins.map(p => `node_modules/${p}`), ...ENV.CHOMP_EJECT ? [] : ['node_modules/@babel/core', 'node_modules/@babel/cli']],
    env,
    run: `babel $DEP -o $TARGET${
        sourceMap ? ' --source-maps' : ''
      }${
        plugins.length ? ` --plugins=${plugins.join(',')}` : ''
      }${
        presets.length ? ` --presets=${presets.join(',')}` : ''
      }${
        !babelRc ? ' --no-babelrc' : ''
      }${
        configFile ? ` --config-file=${configFile.startsWith('./') ? configFile : './' + configFile}` : ''
      }`
  }, ...!babelRc || ENV.CHOMP_EJECT ? [] : ['.babelrc'], ...ENV.CHOMP_EJECT ? [] : [{
    template: 'npm',
    templateOptions: {
      packages: [...presets.map(p => p.startsWith('@babel/') ? p + '@7' : p), ...plugins.map(p => p.startsWith('@babel/') ? p + '@7' : p), '@babel/core@7', '@babel/cli@7'],
      dev: true,
      autoInstall
    }
  }]];
});
