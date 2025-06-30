import { promises as fs } from "fs";
import { compileFromFile } from "json-schema-to-typescript";
import * as path from "path";
import { Project } from "ts-morph";

const CONTRACTS = ["Distributor", "Exchanger", "Manager", "Scheduler", "Twap"];

async function generateTypes() {
  let output = `// AUTO-GENERATED FILE - DO NOT EDIT\n\n`;

  for (const contract of CONTRACTS) {
    const schemaDir = path.join(
      __dirname,
      "../contracts",
      contract.toLowerCase(),
      "schema/raw",
    );

    const files = await fs.readdir(schemaDir);

    for (const file of files) {
      if (!file.endsWith(".json")) continue;

      const schemaPath = path.join(schemaDir, file);
      const ts = await compileFromFile(schemaPath, {
        bannerComment: "",
        customName: (schema, key) => {
          if (schema.title) {
            if (schema.title.endsWith("Msg")) {
              return `${contract}${schema.title}`;
            }
            return schema.title;
          } else {
            return key;
          }
        },
      });

      output += ts;
    }
  }

  const project = new Project({ useInMemoryFileSystem: true });
  const sourceFile = project.createSourceFile("temp.ts", output);

  const emittedTypes = new Set<string>();

  sourceFile.getInterfaces().forEach((iface) => {
    const name = iface.getName();
    if (emittedTypes.has(name)) {
      iface.remove();
    } else {
      emittedTypes.add(name);
    }
  });

  sourceFile.getTypeAliases().forEach((alias) => {
    const name = alias.getName();
    if (emittedTypes.has(name)) {
      alias.remove();
    } else {
      emittedTypes.add(name);
    }
  });

  await fs.writeFile("calc.d.ts", sourceFile.getText());
}

generateTypes().catch((err) => {
  console.error("Error generating types:", err);
  process.exit(1);
});
