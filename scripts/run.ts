import { SigningCosmWasmClient } from "@cosmjs/cosmwasm-stargate";
import { stringToPath } from "@cosmjs/crypto";
import {
  DirectSecp256k1HdWallet,
  DirectSecp256k1Wallet,
} from "@cosmjs/proto-signing";
import { GasPrice } from "@cosmjs/stargate";
import { config } from "dotenv";
import types from "./MsgCompiled";
import { getConfig } from "./script";

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
  "sthor16vaqy5cfkf97p4sfwta0c09epq5tc43p00v3u73esz70x62ghptslcshqq";

getConfig(
  "sthor1xqcawxnfvck4n7qgkstuvxmjeexnnyr9p25544xaxlxcpdlz3teqrq09r6",
).then((c) => console.log(JSON.stringify(c, null, 2)));
