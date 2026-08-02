import { open, stat } from "node:fs/promises";
import { pathToFileURL } from "node:url";

const XZ_MAGIC = Buffer.from([0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]);

export async function verifyEngineArchive(
  archivePath = "src-tauri/resources/engine.tar.xz",
) {
  let metadata;
  try {
    metadata = await stat(archivePath);
  } catch (error) {
    if (error?.code === "ENOENT") {
      throw new Error(
        `缺少正式构建所需的 ${archivePath}。请先运行 \`uv run --locked python scripts/build_engine.py\`。`,
      );
    }
    throw error;
  }

  if (!metadata.isFile() || metadata.size === 0) {
    throw new Error(
      `${archivePath} 不是非空文件。请重新构建或换用已经验证过的引擎归档。`,
    );
  }

  const handle = await open(archivePath, "r");
  try {
    const header = Buffer.alloc(XZ_MAGIC.length);
    const { bytesRead } = await handle.read(header, 0, header.length, 0);
    if (bytesRead !== XZ_MAGIC.length || !header.equals(XZ_MAGIC)) {
      throw new Error(`${archivePath} 不是有效的 XZ 归档。`);
    }
  } finally {
    await handle.close();
  }

  return metadata.size;
}

async function main() {
  const archivePath =
    process.argv[2] ?? "src-tauri/resources/engine.tar.xz";
  const size = await verifyEngineArchive(archivePath);
  console.log(`引擎归档已就绪: ${archivePath} (${size} bytes)`);
}

const invokedAsScript =
  process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;

if (invokedAsScript) {
  main().catch((error) => {
    console.error(`错误: ${error.message}`);
    process.exitCode = 1;
  });
}
