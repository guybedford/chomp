Chomp.registerTemplate('cargo', function ({ deps, env, templateOptions: { bin, install, ...invalid } }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid cargo template option "${Object.keys(invalid)[0]}"`);
  const sep = PATH.match(/\\|\//)[0];
  return ENV.CHOMP_EJECT ? [] : [{
    name: `cargo:${bin}`,
    targets: [PATH.split(';').find(p => p.endsWith(`.cargo${sep}bin`)) + sep + bin + (sep === '/' ? '' : '.exe')],
    invalidation: 'not-found',
    display: false,
    deps,
    env,
    run: `cargo install ${install}`
  }];
});
