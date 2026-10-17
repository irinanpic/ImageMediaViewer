import { execSync } from "child_process";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

/**
 * 独立プラグインバイナリ自動ビルド・配置スクリプト
 *
 * 変更理由: アプリ本体コアとは別に独立してビルドされるプラグインバイナリ（C-ABI動的ライブラリおよびCLIバイナリ）を、
 * Windows（.dll / .exe）、Linux（.so）、macOS（.dylib）、および Android（.so）向けに
 * リリースビルドまたは開発時に自動コンパイルし、plugins/<plugin>/ および Android jniLibs 配下へ確実に配置するため。
 */

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

export const plugins = [
  {
    name: "clipstudio",
    packageName: "imv-plugin-clipstudio",
    srcDir: path.join(rootDir, "plugins-src", "clipstudio"),
    destDir: path.join(rootDir, "plugins", "clipstudio"),
    artifacts: {
      windows: ["imv_clipstudio.dll", "clipstudio_plugin.exe"],
      linux: ["libimv_clipstudio.so", "clipstudio_plugin"],
      macos: ["libimv_clipstudio.dylib", "clipstudio_plugin"],
    },
    androidArtifact: "libimv_clipstudio.so",
  },
];

/**
 * Android SDK パスを自動探索する
 *
 * @returns {string | null} 検出された Android SDK の絶対パス
 */
export function findAndroidHome() {
  if (process.env.ANDROID_HOME && fs.existsSync(process.env.ANDROID_HOME)) {
    return process.env.ANDROID_HOME;
  }
  const localAppData = process.env.LOCALAPPDATA;
  if (localAppData) {
    const candidate = path.join(localAppData, "Android", "Sdk");
    if (fs.existsSync(candidate)) return candidate;
  }
  const home = process.env.HOME || process.env.USERPROFILE;
  if (home) {
    const candidate = path.join(home, "Android", "Sdk");
    if (fs.existsSync(candidate)) return candidate;
  }
  return null;
}

/**
 * Android NDK パスを自動探索する
 *
 * @param {string} [androidHome] Android SDK のパス
 * @returns {string | null} 検出された NDK の絶対パス
 */
export function findNdkHome(androidHome = findAndroidHome()) {
  if (process.env.NDK_HOME && fs.existsSync(process.env.NDK_HOME)) {
    return process.env.NDK_HOME;
  }
  if (!androidHome) return null;

  const ndkDir = path.join(androidHome, "ndk");
  if (fs.existsSync(ndkDir)) {
    const versions = fs
      .readdirSync(ndkDir, { withFileTypes: true })
      .filter((d) => d.isDirectory())
      .map((d) => d.name)
      .sort((a, b) => b.localeCompare(a, undefined, { numeric: true }));

    if (versions.length > 0) {
      return path.join(ndkDir, versions[0]);
    }
  }
  return null;
}

/**
 * デスクトップ（Windows, macOS, Linux）向けプラグインバイナリのビルドおよび配置
 *
 * @returns {string[]} 配置されたアーティファクトの絶対パス一覧
 */
export function buildDesktopPlugins() {
  console.log("=========================================");
  console.log("  Building Independent Plugin Binaries");
  console.log("=========================================\n");

  const copiedList = [];

  for (const plugin of plugins) {
    if (!fs.existsSync(plugin.srcDir)) {
      console.warn(`[WARN] Plugin source directory not found: ${plugin.srcDir}`);
      continue;
    }

    console.log(`[Build] Compiling plugin: ${plugin.name} (${plugin.srcDir})...`);
    try {
      execSync("cargo build --release", {
        cwd: plugin.srcDir,
        stdio: "inherit",
      });
    } catch (err) {
      console.error(`[ERROR] Failed to compile plugin ${plugin.name}:`, err);
      process.exit(1);
    }

    if (!fs.existsSync(plugin.destDir)) {
      fs.mkdirSync(plugin.destDir, { recursive: true });
    }

    const releaseTargetDir = path.join(plugin.srcDir, "target", "release");
    const platform = process.platform === "win32" ? "windows" : process.platform === "darwin" ? "macos" : "linux";
    const artifactsToCopy = plugin.artifacts[platform] || [];

    for (const artifact of artifactsToCopy) {
      const srcPath = path.join(releaseTargetDir, artifact);
      const destPath = path.join(plugin.destDir, artifact);

      if (fs.existsSync(srcPath)) {
        fs.copyFileSync(srcPath, destPath);
        console.log(`[Copy] ${artifact} -> ${destPath}`);
        copiedList.push(destPath);
      } else {
        console.warn(`[WARN] Artifact not found: ${srcPath}`);
      }
    }
  }

  console.log("\n[OK] All desktop plugin binaries built and placed successfully.\n");
  return copiedList;
}

/**
 * Android 向け外部プラグイン動的ライブラリ（.so）の NDK クロスコンパイルおよび jniLibs への自動配置
 *
 * 変更理由: Android 環境においても本体コアに特定形式（ClipStudio等）の知識やフォールバックを持たせず、
 * C-ABI動的ライブラリ（.so）として分離されたプラグインを NDK でコンパイルし、
 * APK 内のネイティブライブラリディレクトリ（jniLibs/arm64-v8a等）へ自動配置して同梱するため。
 *
 * @param {object} [options]
 * @param {string} [options.ndkHome] Android NDK ルートパス（未指定時は自動探索）
 * @param {string} [options.targetArch="aarch64"] ターゲット ("aarch64" または "x86_64")
 * @param {boolean} [options.isRelease=true] リリースビルドか否か
 * @param {string} [options.minSdk="24"] minSdk バージョン
 * @returns {string[]} 配置されたライブラリファイルの絶対パス一覧
 */
export function buildAndroidPlugins({
  ndkHome = findNdkHome(),
  targetArch = "aarch64",
  isRelease = true,
  minSdk = "24",
} = {}) {
  if (!ndkHome) {
    throw new Error("Android NDK が見つかりませんでした。NDK_HOME または ANDROID_HOME を確認してください。");
  }

  // ターゲット別の Rust ターゲット名、ABI名、ツール名プレフィックス
  let rustTarget = "aarch64-linux-android";
  let abi = "arm64-v8a";
  let toolPrefix = "aarch64-linux-android";

  if (targetArch === "x86_64" || targetArch === "x64") {
    rustTarget = "x86_64-linux-android";
    abi = "x86_64";
    toolPrefix = "x86_64-linux-android";
  } else if (targetArch === "armv7" || targetArch === "arm") {
    rustTarget = "armv7-linux-androideabi";
    abi = "armeabi-v7a";
    toolPrefix = "armv7a-linux-androideabi";
  } else if (targetArch === "i686" || targetArch === "x86") {
    rustTarget = "i686-linux-android";
    abi = "x86";
    toolPrefix = "i686-linux-android";
  }

  // LLVM prebuilt ディレクトリの探索
  const prebuiltBase = path.join(ndkHome, "toolchains", "llvm", "prebuilt");
  if (!fs.existsSync(prebuiltBase)) {
    throw new Error(`NDK toolchains/llvm/prebuilt ディレクトリが存在しません: ${prebuiltBase}`);
  }
  const prebuiltHosts = fs.readdirSync(prebuiltBase).filter((d) =>
    fs.statSync(path.join(prebuiltBase, d)).isDirectory()
  );
  if (prebuiltHosts.length === 0) {
    throw new Error(`NDK LLVM prebuilt ホストディレクトリが見つかりません: ${prebuiltBase}`);
  }
  const llvmBin = path.join(prebuiltBase, prebuiltHosts[0], "bin");

  const isWin = process.platform === "win32";
  const clangCmd = path.join(llvmBin, `${toolPrefix}${minSdk}-clang${isWin ? ".cmd" : ""}`);
  const clangxxCmd = path.join(llvmBin, `${toolPrefix}${minSdk}-clang++${isWin ? ".cmd" : ""}`);
  const arExe = path.join(llvmBin, `llvm-ar${isWin ? ".exe" : ""}`);

  if (!fs.existsSync(clangCmd)) {
    throw new Error(`NDK Clang コンパイラが見つかりません: ${clangCmd}`);
  }

  // Cargo 向け環境変数のセットアップ
  const envKeyTarget = rustTarget.toUpperCase().replace(/-/g, "_");
  const env = {
    ...process.env,
    [`CARGO_TARGET_${envKeyTarget}_LINKER`]: clangCmd,
    [`CC_${rustTarget.replace(/-/g, "_")}`]: clangCmd,
    [`CXX_${rustTarget.replace(/-/g, "_")}`]: clangxxCmd,
    [`AR_${rustTarget.replace(/-/g, "_")}`]: arExe,
  };

  const copiedArtifacts = [];

  for (const plugin of plugins) {
    if (!fs.existsSync(plugin.srcDir)) {
      console.warn(`[WARN] Plugin source directory not found: ${plugin.srcDir}`);
      continue;
    }

    console.log(`[Android Plugin Build] Compiling ${plugin.name} for ${rustTarget} (${isRelease ? "Release" : "Debug"})...`);
    const cargoCmd = `cargo build --package ${plugin.packageName} --target ${rustTarget} ${isRelease ? "--release" : ""}`;
    try {
      execSync(cargoCmd, {
        cwd: plugin.srcDir,
        env,
        stdio: "inherit",
      });
    } catch (err) {
      console.error(`[ERROR] Failed to compile Android plugin ${plugin.name}:`, err);
      throw err;
    }

    const buildType = isRelease ? "release" : "debug";
    const artifactName = plugin.androidArtifact || `libimv_${plugin.name}.so`;
    const srcSo = path.join(plugin.srcDir, "target", rustTarget, buildType, artifactName);

    if (!fs.existsSync(srcSo)) {
      console.warn(`[WARN] Compiled plugin library not found: ${srcSo}`);
      continue;
    }

    // 1. src-tauri/gen/android/app/src/main/jniLibs/<abi>/ へ配置（APK同梱用）
    const jniDestDir = path.join(rootDir, "src-tauri", "gen", "android", "app", "src", "main", "jniLibs", abi);
    if (!fs.existsSync(jniDestDir)) {
      fs.mkdirSync(jniDestDir, { recursive: true });
    }
    const jniDestPath = path.join(jniDestDir, artifactName);
    fs.copyFileSync(srcSo, jniDestPath);
    console.log(`[Android Plugin Copy] ${srcSo} -> ${jniDestPath}`);
    copiedArtifacts.push(jniDestPath);

    // 2. plugins/<plugin>/ 配下へも最新バイナリを同期
    if (!fs.existsSync(plugin.destDir)) {
      fs.mkdirSync(plugin.destDir, { recursive: true });
    }
    const pluginDestPath = path.join(plugin.destDir, artifactName);
    fs.copyFileSync(srcSo, pluginDestPath);
    console.log(`[Android Plugin Copy] ${srcSo} -> ${pluginDestPath}`);
  }

  return copiedArtifacts;
}

// CLI直接実行時のエントリポイント
if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(__filename)) {
  const args = process.argv.slice(2);
  const isAndroid = args.includes("--android");
  const isAll = args.includes("--all");
  const isRelease = !args.includes("--debug");

  let targetArch = "aarch64";
  const targetIdx = args.indexOf("--target");
  if (targetIdx !== -1 && args[targetIdx + 1]) {
    targetArch = args[targetIdx + 1];
  } else if (args.includes("--x86_64")) {
    targetArch = "x86_64";
  }

  if (isAndroid || isAll) {
    try {
      if (targetArch === "all") {
        console.log("[Android] Building all supported target architectures (aarch64, x86_64)...");
        buildAndroidPlugins({ targetArch: "aarch64", isRelease });
        buildAndroidPlugins({ targetArch: "x86_64", isRelease });
      } else {
        buildAndroidPlugins({ targetArch, isRelease });
      }
    } catch (err) {
      console.error(err.message);
      process.exit(1);
    }
  }

  if (!isAndroid || isAll) {
    buildDesktopPlugins();
  }
}
