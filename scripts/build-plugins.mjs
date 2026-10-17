import { execSync } from "child_process";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

/**
 * 独立プラグインバイナリ自動ビルド・配置・個別パッケージ生成スクリプト
 *
 * 変更理由: 特定プラグイン名（clipstudio等）のハードコードを廃止し、plugins-src/ 配下の各プラグインを
 * 自動検出してビルド・配置するとともに、単体配布用のZIPパッケージ（dist-plugins/*.zip）を生成できるようにするため。
 */

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

/**
 * plugins-src/ 配下のプラグインプロジェクトを自動検出する
 *
 * @param {string} [filterName] 特定のプラグイン名のみに絞り込む場合に指定
 * @returns {Array<{name: string, packageName: string, version: string, srcDir: string, destDir: string, artifacts: object, androidArtifact: string}>}
 */
export function discoverPluginProjects(filterName = null) {
  const pluginsSrcRoot = path.join(rootDir, "plugins-src");
  if (!fs.existsSync(pluginsSrcRoot)) {
    return [];
  }

  const entries = fs.readdirSync(pluginsSrcRoot, { withFileTypes: true });
  const result = [];

  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const name = entry.name;
    if (filterName && name !== filterName) continue;

    const srcDir = path.join(pluginsSrcRoot, name);
    const cargoTomlPath = path.join(srcDir, "Cargo.toml");
    const manifestPath = path.join(srcDir, "plugin.json");

    if (!fs.existsSync(cargoTomlPath)) continue;

    const cargoContent = fs.readFileSync(cargoTomlPath, "utf8");
    const pkgNameMatch = cargoContent.match(/^\s*name\s*=\s*"([^"]+)"/m);
    const versionMatch = cargoContent.match(/^\s*version\s*=\s*"([^"]+)"/m);
    const packageName = pkgNameMatch ? pkgNameMatch[1] : `imv-plugin-${name}`;
    const version = versionMatch ? versionMatch[1] : "1.0.0";

    // plugin.json からライブラリ名・実行ファイル名を解決
    let winArtifacts = [];
    let linuxArtifacts = [];
    let macArtifacts = [];
    let androidArtifact = `libimv_${name}.so`;

    if (fs.existsSync(manifestPath)) {
      try {
        const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
        const libCfg = manifest.extractor?.library;
        const exeCfg = manifest.extractor?.executable;

        if (libCfg) {
          if (typeof libCfg === "string") {
            winArtifacts.push(libCfg);
            linuxArtifacts.push(libCfg);
            macArtifacts.push(libCfg);
          } else {
            if (libCfg.windows) winArtifacts.push(libCfg.windows);
            if (libCfg.linux) linuxArtifacts.push(libCfg.linux);
            if (libCfg.macos) macArtifacts.push(libCfg.macos);
            if (libCfg.android) androidArtifact = libCfg.android;
          }
        }
        if (exeCfg) {
          if (typeof exeCfg === "string") {
            winArtifacts.push(exeCfg);
          } else {
            if (exeCfg.windows) winArtifacts.push(exeCfg.windows);
            if (exeCfg.default) {
              linuxArtifacts.push(exeCfg.default);
              macArtifacts.push(exeCfg.default);
            }
          }
        }
      } catch (e) {
        console.warn(`[WARN] Failed to parse ${manifestPath}:`, e.message);
      }
    }

    result.push({
      name,
      packageName,
      version,
      srcDir,
      destDir: path.join(rootDir, "plugins", name),
      artifacts: {
        windows: winArtifacts,
        linux: linuxArtifacts,
        macos: macArtifacts,
      },
      androidArtifact,
    });
  }

  return result;
}

export const plugins = discoverPluginProjects();

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
 * @param {object} [options]
 * @param {string|null} [options.pluginName=null] 対象プラグイン名（nullで全件）
 * @param {boolean} [options.createPackage=false] 個別配布用ZIPパッケージを出力するか否か
 * @returns {string[]} 配置されたアーティファクトの絶対パス一覧
 */
export function buildDesktopPlugins({ pluginName = null, createPackage = false } = {}) {
  console.log("=========================================");
  console.log("  Building Independent Plugin Binaries");
  console.log("=========================================\n");

  const targetPlugins = discoverPluginProjects(pluginName);
  const copiedList = [];
  const platform =
    process.platform === "win32"
      ? "windows"
      : process.platform === "darwin"
      ? "macos"
      : "linux";

  for (const plugin of targetPlugins) {
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

    // 1. plugin.json の同期コピー
    const srcManifest = path.join(plugin.srcDir, "plugin.json");
    const destManifest = path.join(plugin.destDir, "plugin.json");
    if (fs.existsSync(srcManifest)) {
      fs.copyFileSync(srcManifest, destManifest);
      console.log(`[Copy] plugin.json -> ${destManifest}`);
    }

    // 2. ビルド済みバイナリのコピー
    const releaseTargetDir = path.join(plugin.srcDir, "target", "release");
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

    // 3. 個別配布用パッケージ（ZIP）の生成
    if (createPackage) {
      packageStandalonePlugin(plugin, platform);
    }
  }

  console.log("\n[OK] All desktop plugin binaries built and placed successfully.\n");
  return copiedList;
}

/**
 * プラグインを単体配布可能なZIPアーカイブ（dist-plugins/<name>-plugin-v<version>-<os>.zip）としてパッケージングする
 *
 * @param {object} plugin プラグイン定義オブジェクト
 * @param {string} platform OS識別子 ("windows" | "macos" | "linux")
 * @returns {string | null} 生成されたZIPファイルのパス
 */
export function packageStandalonePlugin(plugin, platform) {
  const distDir = path.join(rootDir, "dist-plugins");
  const stagingRoot = path.join(distDir, "_staging", plugin.name);

  if (fs.existsSync(stagingRoot)) {
    fs.rmSync(stagingRoot, { recursive: true, force: true });
  }
  fs.mkdirSync(stagingRoot, { recursive: true });

  // plugin.json と README.md をステージングへコピー
  for (const metaFile of ["plugin.json", "README.md"]) {
    const src = path.join(plugin.srcDir, metaFile);
    if (fs.existsSync(src)) {
      fs.copyFileSync(src, path.join(stagingRoot, metaFile));
    }
  }

  // ビルド済みバイナリをステージングへコピー
  const releaseTargetDir = path.join(plugin.srcDir, "target", "release");
  const artifacts = plugin.artifacts[platform] || [];
  for (const artifact of artifacts) {
    const src = path.join(releaseTargetDir, artifact);
    if (fs.existsSync(src)) {
      fs.copyFileSync(src, path.join(stagingRoot, artifact));
    }
  }

  const zipFileName = `${plugin.name}-plugin-v${plugin.version}-${platform}.zip`;
  const zipFilePath = path.join(distDir, zipFileName);
  if (fs.existsSync(zipFilePath)) {
    fs.unlinkSync(zipFilePath);
  }

  try {
    execSync(`tar -a -cf "${zipFilePath}" -C "${path.join(distDir, "_staging")}" "${plugin.name}"`, {
      stdio: "inherit",
    });
    console.log(`[Package] Standalone plugin archive created: ${zipFilePath}`);
  } catch (_) {
    if (process.platform === "win32") {
      execSync(
        `powershell -NoProfile -NonInteractive -Command "Compress-Archive -Path '${stagingRoot}' -DestinationPath '${zipFilePath}' -Force"`,
        { stdio: "inherit" }
      );
      console.log(`[Package] Standalone plugin archive created (PowerShell): ${zipFilePath}`);
    }
  } finally {
    fs.rmSync(path.join(distDir, "_staging"), { recursive: true, force: true });
  }

  return zipFilePath;
}

/**
 * Android 向け外部プラグイン動的ライブラリ（.so）の NDK クロスコンパイルおよび jniLibs / assets への自動配置
 *
 * 変更理由: Android 環境において Windows 用の .dll / .exe 等が assets に混入するのを防ぎ、
 * クリーンに plugin.json のみを assets/plugins/<name>/ に、.so のみを jniLibs/<abi>/ に配置するため。
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

  const envKeyTarget = rustTarget.toUpperCase().replace(/-/g, "_");
  const env = {
    ...process.env,
    [`CARGO_TARGET_${envKeyTarget}_LINKER`]: clangCmd,
    [`CC_${rustTarget.replace(/-/g, "_")}`]: clangCmd,
    [`CXX_${rustTarget.replace(/-/g, "_")}`]: clangxxCmd,
    [`AR_${rustTarget.replace(/-/g, "_")}`]: arExe,
  };

  const copiedArtifacts = [];
  const targetPlugins = discoverPluginProjects();

  for (const plugin of targetPlugins) {
    if (!fs.existsSync(plugin.srcDir)) {
      console.warn(`[WARN] Plugin source directory not found: ${plugin.srcDir}`);
      continue;
    }

    console.log(`[Android Plugin Build] Compiling ${plugin.name} for ${rustTarget} (${isRelease ? "Release" : "Debug"})...`);
    const cargoCmd = `cargo build --package ${plugin.packageName} --lib --target ${rustTarget} ${isRelease ? "--release" : ""}`;
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

    // 1. src-tauri/gen/android/app/src/main/jniLibs/<abi>/ へ .so のみを配置（APK同梱用）
    const jniDestDir = path.join(rootDir, "src-tauri", "gen", "android", "app", "src", "main", "jniLibs", abi);
    if (!fs.existsSync(jniDestDir)) {
      fs.mkdirSync(jniDestDir, { recursive: true });
    }
    const jniDestPath = path.join(jniDestDir, artifactName);
    fs.copyFileSync(srcSo, jniDestPath);
    console.log(`[Android Plugin Copy] ${srcSo} -> ${jniDestPath}`);
    copiedArtifacts.push(jniDestPath);

    // 2. Android assets/plugins/<name>/ には plugin.json のみを配置（Windows用 .dll/.exe を混入させない）
    const androidAssetPluginDir = path.join(
      rootDir,
      "src-tauri",
      "gen",
      "android",
      "app",
      "src",
      "main",
      "assets",
      "plugins",
      plugin.name
    );
    if (!fs.existsSync(androidAssetPluginDir)) {
      fs.mkdirSync(androidAssetPluginDir, { recursive: true });
    }
    const srcManifest = path.join(plugin.srcDir, "plugin.json");
    if (fs.existsSync(srcManifest)) {
      const assetManifestPath = path.join(androidAssetPluginDir, "plugin.json");
      fs.copyFileSync(srcManifest, assetManifestPath);
      console.log(`[Android Asset Copy] ${srcManifest} -> ${assetManifestPath}`);
    }
  }

  return copiedArtifacts;
}

// CLI直接実行時のエントリポイント
if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(__filename)) {
  const args = process.argv.slice(2);
  const isAndroid = args.includes("--android");
  const isAll = args.includes("--all");
  const isRelease = !args.includes("--debug");
  const createPackage = args.includes("--package");

  let pluginName = null;
  const pluginIdx = args.indexOf("--plugin");
  if (pluginIdx !== -1 && args[pluginIdx + 1]) {
    pluginName = args[pluginIdx + 1];
  }

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
    buildDesktopPlugins({ pluginName, createPackage });
  }
}
