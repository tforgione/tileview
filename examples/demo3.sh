#!/usr/bin/env bash

for c in a b c d  e f g h i j k l m n o p q r s t u v w x y z A B C D  E F G H I J K L M N O P Q R S T U V W X Y Z 0 1 2 3 4 5 6 7 8 9; do
for i in `seq 1 $(stty size | cut -d ' ' -f 2)`; do
    echo -n $c
done
sleep 2s
done
