import { SigningCosmWasmClient } from "@cosmjs/cosmwasm-stargate";
import { stringToPath } from "@cosmjs/crypto";
import {
  DirectSecp256k1HdWallet,
  DirectSecp256k1Wallet,
} from "@cosmjs/proto-signing";
import { GasPrice } from "@cosmjs/stargate";
import { config } from "dotenv";
import { Effect } from "effect/index";
import { Addr, Affiliate, CreateTriggerMsg, Node } from "../calc";
import types from "./MsgCompiled";

(BigInt.prototype as any).toJSON = function () {
  return this.toString();
};

config();

export const getWalletWithMnemonic = async () =>
  DirectSecp256k1HdWallet.fromMnemonic(process.env.MNEMONIC!, {
    prefix: process.env.PREFIX! || "sthor",
    hdPaths: [stringToPath(`m/44'/931'/0'/0/0`)],
  });

export const getWalletWithPrivateKey = async () =>
  DirectSecp256k1Wallet.fromKey(
    Buffer.from(process.env.PRIVATE_KEY, "hex"),
    process.env.PREFIX || "sthor",
  );

export const getSigner = async () => {
  const signer = await SigningCosmWasmClient.connectWithSigner(
    process.env.RPC_URL!,
    await getWalletWithMnemonic(),
    // await getWalletWithPrivateKey(),
    {
      gasPrice: GasPrice.fromString(process.env.GAS_PRICE || "0.0urune"),
    },
  );

  signer.registry.register("/types.MsgDeposit", types.types.MsgDeposit);
  return signer;
};

export const getAccount = async (wallet: DirectSecp256k1HdWallet) => {
  const accounts = await wallet.getAccounts();
  return accounts[0]?.address;
};

const SIGNER_ADDRESS = "sthor17pfp4qvy5vrmtjar7kntachm0cfm9m9azl3jka";

const MANAGER_CONTRACT_ADDRESS =
  "sthor18e35rm2dwpx3h09p7q7xx8qfvwdsxz2ls92fdfd4j7vh6g55h8ash7gkau";

const SCHEDULER_CONTRACT_ADDRESS =
  "sthor14zd6glgu67mg2ze7ekqtce3r7yjuk846l3982en9y5v6nlh2y5es2llpa6";

const STRATEGY_ADDRESS =
  "sthor194aywxqfx7jepjgn3p6agsgs9nlwpz7y5dh6nnu6z9tdpfy4fghspj06nd";

const createTrigger = (trigger: { create: CreateTriggerMsg }) =>
  Effect.gen(function* () {
    let wallet = yield* Effect.tryPromise(getWalletWithMnemonic);
    let client = yield* Effect.tryPromise(getSigner);

    const signerAddress = yield* Effect.tryPromise(() => getAccount(wallet));
    const response = yield* Effect.tryPromise(() =>
      client.execute(
        signerAddress,
        SCHEDULER_CONTRACT_ADDRESS,
        trigger,
        "auto",
      ),
    );

    console.log("Trigger created:", JSON.stringify(response, null, 2));
  });

const createStrategy = (strategy: {
  instantiate: {
    affiliates: Affiliate[];
    label: string;
    nodes: Node[];
    owner: Addr;
  };
}) =>
  Effect.gen(function* () {
    let wallet = yield* Effect.tryPromise(getWalletWithMnemonic);
    let client = yield* Effect.tryPromise(getSigner);

    const signerAddress = yield* Effect.tryPromise(() => getAccount(wallet));

    const response = yield* Effect.tryPromise(() =>
      client.execute(signerAddress, MANAGER_CONTRACT_ADDRESS, strategy, "auto"),
    );

    console.log("Strategy created:", JSON.stringify(response, null, 2));
  });

// Effect.runPromise(
//   createStrategy({
//     instantiate: {
//       affiliates: [],
//       label: "Test Strategy",
//       nodes: [
//         {
//           condition: {
//             condition: {
//               schedule: {
//                 cadence: {
//                   blocks: {
//                     interval: 10,
//                   },
//                 },
//                 contract_address: MANAGER_CONTRACT_ADDRESS,
//                 execution_rebate: [],
//                 executors: [],
//                 scheduler: SCHEDULER_CONTRACT_ADDRESS,
//               },
//             },
//             index: 0,
//             on_success: 1,
//             on_failure: 2,
//           },
//         },
//         {
//           action: {
//             action: {
//               swap: {
//                 adjustment: "fixed",
//                 maximum_slippage_bps: 9999,
//                 minimum_receive_amount: {
//                   amount: "0",
//                   denom: "x/ruji",
//                 },
//                 routes: [
//                   {
//                     fin: {
//                       pair_address:
//                         "sthor1knzcsjqu3wpgm0ausx6w0th48kvl2wvtqzmvud4hgst4ggutehlseele4r",
//                     },
//                   },
//                 ],
//                 swap_amount: {
//                   amount: "1000",
//                   denom: "rune",
//                 },
//               },
//             },
//             index: 1,
//             next: 2,
//           },
//         },
//         {
//           action: {
//             action: {
//               swap: {
//                 adjustment: "fixed",
//                 maximum_slippage_bps: 9999,
//                 minimum_receive_amount: {
//                   amount: "0",
//                   denom: "rune",
//                 },
//                 routes: [
//                   {
//                     fin: {
//                       pair_address:
//                         "sthor1knzcsjqu3wpgm0ausx6w0th48kvl2wvtqzmvud4hgst4ggutehlseele4r",
//                     },
//                   },
//                 ],
//                 swap_amount: {
//                   amount: "1000",
//                   denom: "x/ruji",
//                 },
//               },
//             },
//             index: 2,
//           },
//         },
//       ],
//       owner: SIGNER_ADDRESS,
//     },
//   }),
// );

// Effect.runPromise(
//   createTrigger({
//     create: {
//       condition: {
//         timestamp_elapsed: "0",
//       },
//       contract_address: MANAGER_CONTRACT_ADDRESS,
//       executors: [],
//       msg: Buffer.from(
//         JSON.stringify({
//           execute: {
//             contract_address: STRATEGY_ADDRESS,
//           },
//         }),
//       ).toBase64(),
//     },
//   }),
// );
