import { spawn, execSync } from "child_process";
import fs from "fs";
import path from "path";
import { cleanAllTargetReadOnly } from "./clean-target-readonly.mjs";
import { buildAndroidPlugins } from "./build-plugins.mjs";

/**
 * モバイル版（Android）APK ビルド自動化スクリプト
 *
 * 変更理由: ユーザーが複雑な環境変数（ANDROID_HOME, NDK_HOME, JAVA_HOME等）や
 * ターゲットアーキテクチャの指定を手動で行うことなく、
 * `npm run build:android` コマンド1発で確実にAPKをビルド・出力できるようにするため。
 */

/**
 * Android SDK パスを自動探索する
 *
 * @returns {string | null} 検出された Android SDK の絶対パス
 */
function findAndroidHome() {
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
 * @param {string} androidHome Android SDK のパス
 * @returns {string | null} 検出された NDK の絶対パス
 */
function findNdkHome(androidHome) {
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
 * JDK / JBR (Java Home) パスを自動探索する
 *
 * @returns {string | null} 検出された JAVA_HOME の絶対パス
 */
function findJavaHome() {
  if (process.env.JAVA_HOME && fs.existsSync(process.env.JAVA_HOME)) {
    return process.env.JAVA_HOME;
  }
  // Android Studio 付属の JBR (Java Runtime) を優先探索
  const programFiles = process.env["ProgramFiles"];
  if (programFiles) {
    const asJbr = path.join(programFiles, "Android", "Android Studio", "jbr");
    if (fs.existsSync(asJbr)) return asJbr;
  }
  return null;
}

/**
 * Windows 環境において Cargo target および Android build ディレクトリ内のファイル/フォルダから
 * ReadOnly 属性を再帰的かつ高速に解除する（AccessDeniedException / autocfg パニック対策）
 */
function runReadOnlyCleanup() {
  cleanAllTargetReadOnly();
}

/**
 * メインビルド処理
 */
async function main() {
  runReadOnlyCleanup();
  console.log("=========================================");
  console.log("  ImageMediaViewer Android APK Build");
  console.log("=========================================\n");

  const androidHome = findAndroidHome();
  const ndkHome = findNdkHome(androidHome);
  const javaHome = findJavaHome();

  if (!androidHome) {
    console.error("【エラー】Android SDK が見つかりませんでした。");
    console.error("Android Studio がインストールされているか、環境変数 ANDROID_HOME を設定してください。");
    process.exit(1);
  }

  if (!ndkHome) {
    console.error("【エラー】Android NDK が見つかりませんでした。");
    console.error("Android Studio の SDK Manager より NDK をインストールしてください。");
    process.exit(1);
  }

  if (!javaHome) {
    console.error("【エラー】Java (JAVA_HOME / JBR) が見つかりませんでした。");
    console.error("Android Studio または JDK がインストールされているか確認してください。");
    process.exit(1);
  }

  console.log(`[OK] ANDROID_HOME: ${androidHome}`);
  console.log(`[OK] NDK_HOME:     ${ndkHome}`);
  console.log(`[OK] JAVA_HOME:    ${javaHome}`);

  // 環境変数の準備
  const env = { ...process.env };
  env.ANDROID_HOME = androidHome;
  env.NDK_HOME = ndkHome;
  env.JAVA_HOME = javaHome;

  // プロジェクトディレクトリ外の AppData に target を逃がして肥大化を防止
  const localAppData = process.env.LOCALAPPDATA || process.env.TEMP || "";
  if (localAppData && !env.CARGO_TARGET_DIR) {
    env.CARGO_TARGET_DIR = path.join(localAppData, "ImageMediaViewer", "target");
  }
  runReadOnlyCleanup();

  // PATH に Java と Android ツールを追加
  const pathParts = [];
  const javaBin = path.join(javaHome, "bin");
  if (fs.existsSync(javaBin)) pathParts.push(javaBin);
  const platformTools = path.join(androidHome, "platform-tools");
  if (fs.existsSync(platformTools)) pathParts.push(platformTools);
  const cmdlineTools = path.join(androidHome, "cmdline-tools", "latest", "bin");
  if (fs.existsSync(cmdlineTools)) pathParts.push(cmdlineTools);

  if (env.Path) {
    pathParts.push(env.Path);
    env.Path = pathParts.join(path.delimiter);
  } else if (env.PATH) {
    pathParts.push(env.PATH);
    env.PATH = pathParts.join(path.delimiter);
  }

  // コマンドライン引数の解析
  const userArgs = process.argv.slice(2);
  const isRelease = userArgs.includes("--release");
  let target = "aarch64"; // 実機・ARMエミュレータ向けデフォルト

  const targetIdx = userArgs.indexOf("--target");
  if (targetIdx !== -1 && userArgs[targetIdx + 1]) {
    target = userArgs[targetIdx + 1];
  } else if (userArgs.includes("--x86_64") || userArgs.includes("-x86_64")) {
    target = "x86_64";
  }

  console.log(`[Config] Target Arch: ${target}`);
  console.log(`[Config] Mode:        ${isRelease ? "Release" : "Debug"}\n`);

  // 外部プラグインバイナリ（Android C-ABI動的ライブラリ .so）の事前コンパイルおよび jniLibs 自動配置
  console.log("=========================================");
  console.log("  Android 外部プラグインバイナリのビルド");
  console.log("=========================================\n");
  try {
    buildAndroidPlugins({
      ndkHome,
      targetArch: target,
      isRelease,
    });
  } catch (err) {
    console.error("【エラー】外部プラグインのビルドに失敗しました:", err.message);
    process.exit(1);
  }

  console.log("\nビルドを開始します。しばらくお待ちください...\n");

  const tauriArgs = [
    "tauri",
    "android",
    "build",
    "--apk",
    "--target",
    target,
  ];

  // Tauri CLI の tauri android build はデフォルトが Release モードのため、
  // Debug ビルドの場合のみ --debug フラグを付与する (--release は無効な引数)
  if (!isRelease) {
    tauriArgs.push("--debug");
  }

  // Windows の場合は npx.cmd を呼ぶ
  const isWin = process.platform === "win32";
  const cmd = isWin ? "npx.cmd" : "npx";

  const buildProcess = spawn(cmd, tauriArgs, {
    env,
    stdio: "inherit",
    shell: isWin,
  });

  buildProcess.on("close", (code) => {
    if (code === 0) {
      console.log("\n=========================================");
      console.log("  Android APK のビルドが正常に完了しました！");
      console.log("=========================================\n");

      // 出力先 APK の探索
      const apkBaseDir = path.join(
        process.cwd(),
        "src-tauri",
        "gen",
        "android",
        "app",
        "build",
        "outputs",
        "apk"
      );

      if (fs.existsSync(apkBaseDir)) {
        /**
         * 再帰的に APK ファイルを探索する
         * @param {string} dir 探索対象ディレクトリ
         * @returns {string[]} 見つかったAPKファイル一覧
         */
        function findApks(dir) {
          const results = [];
          for (const item of fs.readdirSync(dir, { withFileTypes: true })) {
            const full = path.join(dir, item.name);
            if (item.isDirectory()) {
              results.push(...findApks(full));
            } else if (item.name.endsWith(".apk")) {
              results.push(full);
            }
          }
          return results;
        }

        let apks = findApks(apkBaseDir);
        if (apks.length > 0) {
          // 未署名 (unsigned) APK がある場合、debug.keystore で自動署名
          const debugKeystore = path.join(
            process.env.USERPROFILE || process.env.HOME || "",
            ".android",
            "debug.keystore"
          );
          const buildToolsDir = path.join(androidHome, "build-tools");
          let apksigner = null;
          if (fs.existsSync(buildToolsDir)) {
            const versions = fs.readdirSync(buildToolsDir).sort().reverse();
            for (const ver of versions) {
              const candidate = path.join(
                buildToolsDir,
                ver,
                process.platform === "win32" ? "apksigner.bat" : "apksigner"
              );
              if (fs.existsSync(candidate)) {
                apksigner = candidate;
                break;
              }
            }
          }

          if (apksigner && fs.existsSync(debugKeystore)) {
            for (const apk of apks) {
              if (apk.endsWith("-unsigned.apk")) {
                const signedApk = apk.replace("-unsigned.apk", ".apk");
                try {
                  console.log(`[署名] 未署名 APK を自動署名しています: ${path.basename(signedApk)}`);
                  execSync(
                    `"${apksigner}" sign --ks "${debugKeystore}" --ks-pass pass:android --ks-key-alias androiddebugkey --key-pass pass:android --out "${signedApk}" "${apk}"`,
                    { stdio: "ignore" }
                  );
                } catch (_) {}
              }
            }
          }

          // 署名後の APK 一覧を再取得
          apks = findApks(apkBaseDir);
          const signedApks = apks.filter((a) => !a.includes("-unsigned"));
          const candidates = signedApks.length > 0 ? signedApks : apks;

          console.log("\n生成された APK ファイル:");
          for (const apk of candidates) {
            console.log(`  - ${apk}`);
          }
          const preferredApk =
            candidates.find((a) => (isRelease ? a.includes("release") : a.includes("debug"))) ||
            candidates[0];

          // APK 内のネイティブライブラリ同梱検証
          const jarExe = path.join(javaHome, "bin", process.platform === "win32" ? "jar.exe" : "jar");
          if (fs.existsSync(jarExe) && preferredApk) {
            try {
              const zipEntries = execSync(`"${jarExe}" tf "${preferredApk}"`, { encoding: "utf8" });
              const bundledPlugins = zipEntries
                .split(/\r?\n/)
                .filter((line) => line.includes("libimv_"));
              if (bundledPlugins.length > 0) {
                console.log("\n[検証OK] APK内に同梱された外部プラグインバイナリ (JNI .so):");
                for (const p of bundledPlugins) {
                  console.log(`  - ${p.trim()}`);
                }
              }

              const bundledManifests = zipEntries
                .split(/\r?\n/)
                .filter((line) => line.includes("assets/plugins/") && line.endsWith("plugin.json"));
              if (bundledManifests.length > 0) {
                console.log("\n[検証OK] APK内に同梱されたプラグインマニフェスト (assets):");
                for (const m of bundledManifests) {
                  console.log(`  - ${m.trim()}`);
                }
              }
            } catch (_) {}
          }

          console.log("\n【インストール手順】");
          console.log("実機またはエミュレータが接続された状態で以下のコマンドを実行してください:");
          console.log(`  adb install -r "${preferredApk}"\n`);
        }
      }
      process.exit(0);
    } else {
      console.error(`\n【ビルド失敗 (終了コード: ${code})】`);
      console.error("\n[ヒント / トラブルシューティング]");
      console.error("1. 「AccessDeniedException」または「Unable to delete directory」が発生した場合:");
      console.error("   Android Studio またはエミュレータがビルドディレクトリ内のファイルをロックしています。");
      console.error("   Android Studio 内の「Run」(▶) ボタンで実行するか、一度 Android Studio を終了してから再実行してください。");
      console.error("2. ビルドキャッシュをクリアしたい場合:");
      console.error("   cd src-tauri/gen/android; ./gradlew.bat clean");
      process.exit(code || 1);
    }
  });

  buildProcess.on("error", (err) => {
    console.error("プロセス起動エラー:", err);
    process.exit(1);
  });
}

main().catch((err) => {
  console.error("予期せぬエラー:", err);
  process.exit(1);
});
