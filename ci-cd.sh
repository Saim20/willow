#!/bin/bash

git add .
git commit -m $1
git push

# copy PLGBUILD, .SRCINFO and willow.install from actual repo
cp ./PKGBUILD ./.SRCINFO ./willow.install ../aur-willow

cd ../aur-willow

# git commit and push
git add .
git commit -m $1
git push
