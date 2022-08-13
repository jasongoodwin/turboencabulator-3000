#!/bin/bash
echo "type, client,tx,amount"
for ((a=1; a <= 500000 ; a++))
do
   client=$((a%100))
   echo "deposit,$client,$a,1.1111"
done
