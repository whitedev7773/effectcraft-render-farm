// Optional asset regeneration only. Rust/MSI builds use the committed outputs.
const fs = require('node:fs');
const path = require('node:path');
const sharp = require('sharp');

async function main() {
  const directory = path.join(__dirname, '..', 'assets', 'app-icon');
  const svg = fs.readFileSync(path.join(directory, 'farm.svg'));
  const sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256];
  const images = [];
  for (const size of sizes) {
    const png = await sharp(svg).resize(size, size).png().toBuffer();
    images.push(png);
    if (size === 256) fs.writeFileSync(path.join(directory, 'farm-256.png'), png);
  }
  const header = Buffer.alloc(6 + sizes.length * 16);
  header.writeUInt16LE(1, 2); // ICO
  header.writeUInt16LE(sizes.length, 4);
  let offset = header.length;
  for (let index = 0; index < sizes.length; index++) {
    const entry = 6 + index * 16;
    header[entry] = sizes[index] === 256 ? 0 : sizes[index];
    header[entry + 1] = header[entry];
    header.writeUInt16LE(1, entry + 4);
    header.writeUInt16LE(32, entry + 6);
    header.writeUInt32LE(images[index].length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += images[index].length;
  }
  fs.writeFileSync(path.join(directory, 'farm.ico'), Buffer.concat([header, ...images]));
  await sharp(svg).resize(512, 512).png().toFile(path.join(directory, 'farm-512.png'));
  console.log('Generated Farm PNGs and ICO (16, 20, 24, 32, 40, 48, 64, 128, 256px).');
}

main().catch(error => { console.error(error); process.exitCode = 1; });
