import { executeStrategy } from "./script";

(BigInt.prototype as any).toJSON = function () {
  return this.toString();
};

// uploadAndMigrateContractSuite();

// uploadAndMigrateStrategyContract().then((r) => console.log(r));

executeStrategy(
  "sthor1ydl83upd2asnnuklhw0schdquavepy6tlp47zl2pcn4q69yhn4ks37mzsm",
).then((r) =>
  console.log(`Strategy executed with result: ${JSON.stringify(r, null, 2)}`),
);

// getConfig(
//   "sthor1z7y08s5wkp89s9fafvtsas76e6yws5ytdfuq6424lad0rac98xmqg3jmx0",
// ).then((r) => console.log(JSON.stringify(r, null, 2)));
