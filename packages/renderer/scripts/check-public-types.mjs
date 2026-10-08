/** R7: inspect the public declaration graph with the installed TS 7 native AST.
 * Never treat a bare package or import() type as an opaque leaf. */
import { API } from 'typescript/unstable/sync';
import { SyntaxKind } from 'typescript/unstable/ast';
import { resolve, dirname } from 'node:path';
import { pathToFileURL } from 'node:url';

export function checkPublicTypes(entry) {
  const api = new API({ cwd: dirname(resolve(entry)) });
  let snapshot;
  try {
    const entryFile = resolve(entry);
    snapshot = api.updateSnapshot({ openFiles: [entryFile] });
    const project = snapshot.getDefaultProjectForFile(entryFile);
    if (!project) throw new Error(`No declaration project: ${entryFile}`);
    const { program, checker } = project;
    const queue = [entryFile],
      seen = new Set();
    while (queue.length) {
      const path = queue.pop();
      const source = program.getSourceFile(path);
      if (!source) throw new Error(`Unresolved public declaration: ${path}`);
      if (seen.has(source.path)) continue;
      seen.add(source.path);
      if (program.getSyntacticDiagnostics(source.fileName).length)
        throw new Error(`Invalid declaration syntax: ${source.fileName}`);
      const literals = [];
      const walk = (node) => {
        if (
          (node.kind === SyntaxKind.ImportDeclaration ||
            node.kind === SyntaxKind.ExportDeclaration) &&
          node.moduleSpecifier
        )
          literals.push(node.moduleSpecifier);
        if (node.kind === SyntaxKind.ImportType && node.argument?.literal)
          literals.push(node.argument.literal);
        if (node.kind === SyntaxKind.ImportEqualsDeclaration && node.moduleReference?.expression)
          literals.push(node.moduleReference.expression);
        node.forEachChild(walk);
      };
      walk(source);
      for (const literal of literals) {
        const specifier = literal.text;
        if (typeof specifier !== 'string')
          throw new Error(`Nonliteral module in ${source.fileName}`);
        if (/^(?:pixi\.js(?:\/|$)|@pixi\/)/.test(specifier))
          throw new Error(`Pixi public type: ${source.fileName} → ${specifier}`);
        const symbol = checker.getSymbolAtLocation(literal);
        const targets =
          symbol?.declarations
            ?.filter((d) => d.kind === SyntaxKind.SourceFile)
            .map((d) => d.path) ?? [];
        if (!targets.length)
          throw new Error(`Unresolved public type module: ${source.fileName} → ${specifier}`);
        queue.push(...targets);
      }
      for (const ref of source.referencedFiles)
        queue.push(resolve(dirname(source.fileName), ref.fileName));
      // No type-reference package may silently bypass the module graph. Current
      // public declarations have none; add proper resolution before allowing any.
      if (source.typeReferenceDirectives.length)
        throw new Error(`Unresolved public type reference: ${source.fileName}`);
      for (const ref of source.libReferenceDirectives) {
        const lib = program
          .getSourceFileNames()
          .find((p) => p.endsWith(`/lib.${ref.fileName}.d.ts`));
        if (!lib || !program.isSourceFileDefaultLibrary(program.getSourceFile(lib)))
          throw new Error(`Unknown standard library: ${ref.fileName}`);
        queue.push(lib);
      }
    }
    return { files: seen.size };
  } finally {
    snapshot?.dispose();
    api.close();
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const { files } = checkPublicTypes(process.argv[2] ?? 'dist/index.d.ts');
    console.log(`公共类型闭包 ${files} 文件零 pixi 引用 ✓`);
  } catch (e) {
    console.error(`库构建守卫失败：${e.message}`);
    process.exitCode = 1;
  }
}
