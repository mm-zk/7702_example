// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.13;

contract Counter {
    uint256 public number;

    function setNumber(uint256 newNumber) public {
        number = newNumber;
    }

    function increment() public {
        number++;
    }

    receive() external payable {}
    fallback() external payable {}
    function transferToSender(uint256 amount) external {
        // Check that the contract has at least 'amount' wei.
        require(
            amount <= address(this).balance,
            "Insufficient contract balance"
        );

        // Convert msg.sender to a payable address and send the ETH.
        payable(msg.sender).transfer(amount);
    }
}
