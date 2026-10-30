import { copyFile, mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';

const source = resolve('helper/target/x86_64-pc-windows-msvc/release/orbit-credential-helper.exe');
const targetDir = resolve('com.orbitos.creator.sdPlugin/bin');
await mkdir(targetDir, { recursive: true });
await copyFile(source, resolve(targetDir, 'orbit-credential-helper.exe'));
