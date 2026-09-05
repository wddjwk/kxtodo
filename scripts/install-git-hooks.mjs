// 把 scripts/git-hooks/ 下版本化的 hook 装进本克隆的 hooks 目录。
// npm install / npm ci 经 package.json 的 prepare 自动触发，也可手动 node scripts/install-git-hooks.mjs。
// 不用 core.hooksPath：那会整体接管 hooks 目录，把别的工具挂在 .git/hooks 里的 hook 全部屏蔽掉。
import { spawnSync } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const sourceDir = join(repoRoot, "scripts", "git-hooks");

function hooksDir() {
	const run = (args) => {
		const r = spawnSync("git", args, { cwd: repoRoot, encoding: "utf8" });
		return r.status === 0 ? r.stdout.trim() : "";
	};
	// 不是 git 检出（如从 tarball 安装）时静默跳过，不能拖垮 npm install。
	// 判据是 git dir 而非 hooks dir：hooks dir 可能还不存在，下面会 mkdir 出来。
	if (!run(["rev-parse", "--absolute-git-dir"])) return null;
	const p = run(["rev-parse", "--git-path", "hooks"]);
	return p ? resolve(repoRoot, p) : null;
}

const dir = hooksDir();
if (!dir) process.exit(0);

mkdirSync(dir, { recursive: true });
for (const name of readdirSync(sourceDir)) {
	copyFileSync(join(sourceDir, name), join(dir, name));
	// unix 侧没有执行位 git 会直接跳过该 hook；Windows 上 chmod 基本是 no-op
	chmodSync(join(dir, name), 0o755);
	console.log(`[git-hooks] installed ${name}`);
}
