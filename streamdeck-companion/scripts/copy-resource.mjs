import { copyFile, mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';

const source = resolve('../public/downloads/com.orbitos.creator.streamDeckPlugin');
const target = resolve('../src-tauri/resources/downloads/com.orbitos.creator.streamDeckPlugin');
await mkdir(resolve('../src-tauri/resources/downloads'), { recursive: true });
await copyFile(source, target);
