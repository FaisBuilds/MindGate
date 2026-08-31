cd ~/Desktop/MindGate

cargo clean
cargo build --release

sudo bash ./installer/uninstall.sh
sudo bash ./installer/install.sh

mindgate doctor
sudo systemctl status mindgated mindgate-watchdog --no-pager