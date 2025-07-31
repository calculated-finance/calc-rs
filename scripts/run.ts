import { uploadAndMigrateContractSuite } from "./script";

(BigInt.prototype as any).toJSON = function () {
  return this.toString();
};

uploadAndMigrateContractSuite();
