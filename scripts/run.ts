import { uploadAndMigrateContractSuite } from "./script";

(BigInt.prototype as any).toJSON = function () {
  return this.toString();
};

// queryPool().then((pool) => console.log(JSON.stringify(pool, null, 2)));
// getOrder(PAIR_ADDRESS, KEEPER_ADDRESS, "base", "1.00").then((order) =>
//   console.log(JSON.stringify(order, null, 2)),
// );

uploadAndMigrateContractSuite().then(console.log);

// getConfig(
//   "sthor17rrm9t5e6tr3hycxfhg6x92pfpvccqs2wcgkpu90mppwshs83xrqs3ncx4",
// ).then((r) => console.log(JSON.stringify(r, null, 2)));

// updateStrategy(
//   "sthor17rrm9t5e6tr3hycxfhg6x92pfpvccqs2wcgkpu90mppwshs83xrqs3ncx4",
//   {
//     owner: "sthor17pfp4qvy5vrmtjar7kntachm0cfm9m9azl3jka",
//     state: null,
//     action: {
//       schedule: {
//         scheduler:
//           "sthor1x3hfzl0v43upegeszz8cjygljgex9jtygpx4l44nkxudxjsukn3setrkl6",
//         cadence: {
//           cron: {
//             expr: "*/30 * * * * *",
//             previous: "1753776570000000000",
//           },
//         },
//         execution_rebate: [],
//         action: {
//           swap: {
//             swap_amount: {
//               denom: "x/ruji",
//               amount: "10000000",
//             },
//             minimum_receive_amount: {
//               denom: "rune",
//               amount: "0",
//             },
//             maximum_slippage_bps: 300,
//             adjustment: "fixed",
//             routes: [
//               {
//                 fin: {
//                   pair_address:
//                     "sthor1knzcsjqu3wpgm0ausx6w0th48kvl2wvtqzmvud4hgst4ggutehlseele4r",
//                 },
//               },
//             ],
//           },
//         },
//       },
//     },
//   },
// ).then((r) => console.log(JSON.stringify(r, null, 2)));
