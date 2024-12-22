# Example of 7702

Example of running 7702 transactions.

Accounts used:

## Deployer
Successfully created new keypair.
Address:     0x0A265d1d68fD54B09D434de7846d8E631668d99B
Private key: 0x0fad2ca996a24d116097c481c27a59652a3d3611dfed64d8f9bf86568b1f431d

## EOA
Address:     0x2d9dcCc30D1687EAd032a6fADC5A25776e433080
Private key: 0x411bdd63dc116ba53e0e3fbe752ba21f869e272d4f544c8d545c617ce43f654e


(this is just example code, all private keys there are randomly generated).

Steps:

* get geth 

```shell
git clone git@github.com:lightclient/go-ethereum.git
git checkout prague-devnet-4
make geth
```

* start geth & transfer some assets to deployer:

```shell
./build/bin/geth --dev --http --http.port 8848 console

eth.sendTransaction({from: eth.accounts[0], to: "0x0A265d1d68fD54B09D434de7846d8E631668d99B", value: web3.toWei(100, "ether")});
```

* build & deploy contract: 

```shell
forge script script/Counter.s.sol --rpc-url http://localhost:8848 --private-key 0x0fad2ca996a24d116097c481c27a59652a3d3611dfed64d8f9bf86568b1f431d --broadcast
```

* run the tool - make sure to paste the address from the script above

```shell
cargo run
```

It will set the EOA address (0x2d9dcCc30D1687EAd032a6fADC5A25776e433080) to be running the Counter.sol code.


You can check it by calling:

```shell
cast call -r http://localhost:8848 0x2d9dcCc30D1687EAd032a6fADC5A25776e433080 "number()"
```