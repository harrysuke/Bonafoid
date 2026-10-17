const fs = require('fs');
const files = ['crates/codegen/src/frontend.rs','crates/codegen/src/backend.rs','crates/codegen/src/database.rs'];
for (const f of files) {
  if (fs.existsSync(f)) {
    let c = fs.readFileSync(f, 'utf8');
    c = c.replace(/include_str\(/g, 'include_str!(');
    fs.writeFileSync(f, c);
    console.log('fixed ' + f);
  }
}
