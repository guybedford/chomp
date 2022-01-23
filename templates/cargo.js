Chomp.registerTemplate('cargo', function ({ deps, env, templateOptions: { bin, install } }, { PATH, CHOMP_EJECT }) {
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
