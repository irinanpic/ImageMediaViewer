import { describe, expect, it } from "vitest";
import fs from "fs";
import path from "path";
import ts from "typescript";

/**
 * フック呼び出し規約（Rules of Hooks）の厳格な静的AST解析エンジン
 *
 * 変更理由: React エラー #310 ("Rendered more hooks than during the previous render") の
 * 再発を恒久的に防止するため。
 * Reactの公式Rules of Hooksに基づき、以下を包括的に自動検証する：
 * 1. 早期returnの後にフックが呼び出されていないこと
 * 2. if/else、三項演算子、switch、短絡評価論理演算（&&, ||, ??）などの条件分岐内でフックが呼び出されていないこと
 * 3. for/whileなどのループ文内でフックが呼び出されていないこと
 * 4. イベントハンドラー、コールバック、無名関数などの非コンポーネント／非フック関数内でフックが呼び出されていないこと
 * 5. 他の関数やフックの引数式内でフックが直接呼び出されていないこと（React.memo等のHOCラッパーを除く）
 */
export const isHook = (text: string): boolean => {
  if (!text) return false;
  if (text.startsWith("useAppStore.getState") || text.startsWith("useAppStore.setState")) return false;
  return /\buse[A-Z0-9]/.test(text) || /\bReact\.use[A-Z0-9]/.test(text);
};

export const isComponentOrHookName = (name: string): boolean => {
  if (!name) return false;
  return /^[A-Z]/.test(name) || /^use[A-Z0-9]/.test(name);
};

export const analyzeSource = (code: string, fileName: string = "test.tsx"): string[] => {
  const sf = ts.createSourceFile(fileName, code, ts.ScriptTarget.Latest, true);
  const violations: string[] = [];

  interface WalkState {
    currentFunction: string | null;
    inConditional: string | null;
    afterReturn: boolean;
    returnLine: number;
    inArgumentOf: string | null;
  }

  const hasReturn = (node: ts.Node): boolean => {
    if (ts.isReturnStatement(node)) return true;
    if (ts.isFunctionDeclaration(node) || ts.isFunctionExpression(node) || ts.isArrowFunction(node)) {
      return false;
    }
    let found = false;
    ts.forEachChild(node, (child) => {
      if (hasReturn(child)) found = true;
    });
    return found;
  };

  const checkNode = (node: ts.Node, state: WalkState) => {
    // 1. フック呼び出し判定
    if (ts.isCallExpression(node)) {
      const exprText = node.expression.getText(sf);
      if (isHook(exprText)) {
        const line = sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1;

        if (!state.currentFunction || !isComponentOrHookName(state.currentFunction)) {
          violations.push(
            `非コンポーネント・非フック関数 (${state.currentFunction || "無名関数"}) 内でフック ${exprText}（行${line}）が呼ばれています: ${fileName}`
          );
        }

        if (state.inConditional) {
          violations.push(
            `条件分岐 (${state.inConditional}) 内でフック ${exprText}（行${line}）が呼ばれています: ${fileName}`
          );
        }

        if (state.afterReturn) {
          violations.push(
            `早期return（行${state.returnLine}）の後にフック ${exprText}（行${line}）が呼ばれています: ${fileName}`
          );
        }

        const hocWrappers = ["React.memo", "memo", "React.forwardRef", "forwardRef"];
        if (state.inArgumentOf && !hocWrappers.includes(state.inArgumentOf)) {
          violations.push(
            `関数 ${state.inArgumentOf} の引数内でフック ${exprText}（行${line}）が呼ばれています: ${fileName}`
          );
        }
      }
    }

    // 2. 関数定義（コンポーネント、カスタムフック、イベントハンドラーなど）
    if (ts.isFunctionDeclaration(node) || ts.isFunctionExpression(node) || ts.isArrowFunction(node)) {
      let fnName = "";
      if (node.name) {
        fnName = node.name.getText(sf);
      } else if (node.parent && ts.isVariableDeclaration(node.parent) && node.parent.name) {
        fnName = node.parent.name.getText(sf);
      } else if (
        node.parent &&
        ts.isCallExpression(node.parent) &&
        (node.parent.expression.getText(sf).includes("memo") || node.parent.expression.getText(sf).includes("forwardRef"))
      ) {
        if (node.parent.parent && ts.isVariableDeclaration(node.parent.parent) && node.parent.parent.name) {
          fnName = node.parent.parent.name.getText(sf);
        }
      }

      const nextState: WalkState = {
        currentFunction: fnName || state.currentFunction || "anonymous",
        inConditional: null,
        afterReturn: false,
        returnLine: 0,
        inArgumentOf: null,
      };

      if (node.body && ts.isBlock(node.body)) {
        let sawReturn = false;
        let returnLine = 0;
        for (const stmt of node.body.statements) {
          checkNode(stmt, {
            ...nextState,
            afterReturn: isComponentOrHookName(fnName) ? sawReturn : false,
            returnLine,
          });
          if (hasReturn(stmt)) {
            sawReturn = true;
            returnLine = sf.getLineAndCharacterOfPosition(stmt.getStart(sf)).line + 1;
          }
        }
      } else if (node.body) {
        checkNode(node.body, nextState);
      }
      return;
    }

    // 3. 条件分岐判定
    if (ts.isIfStatement(node)) {
      checkNode(node.expression, state);
      checkNode(node.thenStatement, { ...state, inConditional: "if文" });
      if (node.elseStatement) {
        checkNode(node.elseStatement, { ...state, inConditional: "else文" });
      }
      return;
    }

    if (ts.isConditionalExpression(node)) {
      checkNode(node.condition, state);
      checkNode(node.whenTrue, { ...state, inConditional: "三項演算子(true)" });
      checkNode(node.whenFalse, { ...state, inConditional: "三項演算子(false)" });
      return;
    }

    if (ts.isIterationStatement(node, false)) {
      checkNode(node.statement, { ...state, inConditional: "ループ文" });
      return;
    }

    if (ts.isSwitchStatement(node)) {
      checkNode(node.expression, state);
      for (const clause of node.caseBlock.clauses) {
        for (const s of clause.statements) {
          checkNode(s, { ...state, inConditional: "switch文" });
        }
      }
      return;
    }

    if (ts.isBinaryExpression(node)) {
      const op = node.operatorToken.kind;
      if (
        op === ts.SyntaxKind.AmpersandAmpersandToken ||
        op === ts.SyntaxKind.BarBarToken ||
        op === ts.SyntaxKind.QuestionQuestionToken
      ) {
        checkNode(node.left, state);
        checkNode(node.right, { ...state, inConditional: `論理式(${node.operatorToken.getText(sf)})` });
        return;
      }
    }

    // 4. 関数呼び出し（引数内フック検出用）
    if (ts.isCallExpression(node)) {
      const callName = node.expression.getText(sf);
      checkNode(node.expression, state);
      for (const arg of node.arguments) {
        checkNode(arg, { ...state, inArgumentOf: callName });
      }
      return;
    }

    ts.forEachChild(node, (child) => checkNode(child, state));
  };

  const initialState: WalkState = {
    currentFunction: null,
    inConditional: null,
    afterReturn: false,
    returnLine: 0,
    inArgumentOf: null,
  };

  ts.forEachChild(sf, (node) => checkNode(node, initialState));
  return violations;
};

export const analyzeFile = (filePath: string): string[] => {
  const code = fs.readFileSync(filePath, "utf8");
  return analyzeSource(code, path.basename(filePath));
};

describe("Rules of Hooks 静的検証", () => {
  it("解析エンジン自身が各違反パターン（早期return後、三項演算子、短絡評価、コールバック内、引数内）を正しく検出すること", () => {
    // 早期return後のフック
    const codeAfterReturn = `
      function BadComp({ condition }: { condition: boolean }) {
        if (condition) return null;
        const [state] = useState(0);
        return <div>{state}</div>;
      }
    `;
    expect(analyzeSource(codeAfterReturn)).toHaveLength(1);

    // 三項演算子内のフック
    const codeTernary = `
      function BadComp({ condition }: { condition: boolean }) {
        const val = condition ? useMemo(() => 1, []) : 2;
        return <div>{val}</div>;
      }
    `;
    expect(analyzeSource(codeTernary)).toHaveLength(1);

    // 短絡評価論理演算内のフック
    const codeLogical = `
      function BadComp({ condition }: { condition: boolean }) {
        const val = condition && useMemo(() => 1, []);
        return <div>{String(val)}</div>;
      }
    `;
    expect(analyzeSource(codeLogical)).toHaveLength(1);

    // コールバック・イベントハンドラー内のフック
    const codeCallback = `
      function BadComp() {
        const handleClick = () => {
          useState(0);
        };
        return <button onClick={handleClick} />;
      }
    `;
    expect(analyzeSource(codeCallback)).toHaveLength(1);

    // 引数内直接フック呼び出し
    const codeArg = `
      function BadComp() {
        const val = useAppStore(useCallback(() => true, []));
        return <div>{String(val)}</div>;
      }
    `;
    expect(analyzeSource(codeArg)).toHaveLength(1);
  });

  it("GridCell.tsx においてフック規約違反が存在しないこと", () => {
    const gridCellPath = path.resolve(__dirname, "../GridCell.tsx");
    const violations = analyzeFile(gridCellPath);
    expect(violations).toEqual([]);
  });

  it("src配下の全コンポーネントおよびカスタムフックでフック規約違反が完全にゼロであること", () => {
    const srcDir = path.resolve(__dirname, "../../../");
    const allViolations: string[] = [];

    const walkDir = (dir: string) => {
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const full = path.join(dir, entry.name);
        if (entry.isDirectory() && entry.name !== "node_modules" && entry.name !== ".git" && entry.name !== "dist") {
          walkDir(full);
        } else if (
          entry.isFile() &&
          (entry.name.endsWith(".tsx") || entry.name.endsWith(".ts")) &&
          !entry.name.endsWith(".test.ts") &&
          !entry.name.endsWith(".test.tsx")
        ) {
          const v = analyzeFile(full);
          allViolations.push(...v);
        }
      }
    };

    walkDir(srcDir);
    expect(allViolations).toEqual([]);
  });
});
