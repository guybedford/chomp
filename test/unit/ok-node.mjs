import { writeFileSync } from 'fs';

writeFileSync('output/unittest.txt', 'UNIT OK');

console.log('THIS SHOULD NEVER DISPLAY WHEN RUNNING TESTS');
