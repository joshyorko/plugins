import { mkdir, copyFile } from "node:fs/promises";
import { dirname, join } from "node:path";
const target = join(import.meta.dirname, "dist", "control-room.html");
await mkdir(dirname(target), { recursive: true });
await copyFile(join(import.meta.dirname, "../ui/control-room.html"), target);
console.log(`built ${target}`);
