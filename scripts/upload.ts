import { SigningCosmWasmClient } from "@cosmjs/cosmwasm-stargate";
import { stringToPath } from "@cosmjs/crypto";
import { DirectSecp256k1HdWallet, type Coin } from "@cosmjs/proto-signing";
import { config } from "dotenv";
import fs from "fs";

const getWallet = async () =>
  DirectSecp256k1HdWallet.fromMnemonic(process.env.MNEMONIC!, {
    prefix: process.env.PREFIX! || "sthor",
    hdPaths: [stringToPath(`m/44'/931'/0'/0/0`)],
  });

const getSigner = async () =>
  SigningCosmWasmClient.connectWithSigner(
    process.env.RPC_URL!,
    await getWallet()
  );

export const uploadAndInstantiate = async (
  binaryFilePath: string,
  adminAddress: string,
  initMsg: Record<string, unknown>,
  label: string,
  funds: Coin[] = []
): Promise<string> => {
  const cosmWasmClient = await getSigner();

  const { codeId } = await cosmWasmClient.upload(
    adminAddress,
    fs.readFileSync(binaryFilePath),
    1.5
  );

  console.log("Uploaded code id:", codeId);

  const { contractAddress } = await cosmWasmClient.instantiate(
    adminAddress,
    codeId,
    initMsg,
    label,
    1.5,
    { funds, admin: adminAddress }
  );

  console.log(label, "contract address:", contractAddress);

  return contractAddress;
};

export const getAccount = async () => {
  const wallet = await DirectSecp256k1HdWallet.fromMnemonic(
    process.env.MNEMONIC!,
    {
      prefix: "sthor",
      hdPaths: [stringToPath("m/44'/931'/0'/0/0")],
    }
  );

  const accounts = await wallet.getAccounts();
  return accounts[0]?.address;
};

config();
getAccount().then(console.log).catch(console.error);
