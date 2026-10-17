const fs = require('fs');
const files = [
  'crates/model/src/page.rs',
  'crates/model/src/workflow.rs',
  'crates/model/src/form.rs',
  'crates/model/src/entity.rs',
  'crates/model/src/theme.rs',
  'crates/model/src/project.rs',
  'crates/model/src/lib.rs',
];
for (const f of files) {
  if (!fs.existsSync(f)) continue;
  let c = fs.readFileSync(f, 'utf8');
  const before = c;
  c = c.replace(/r#r#/g, 'r#');
  if (c !== before) { fs.writeFileSync(f, c); console.log('Fixed', f); }
}
