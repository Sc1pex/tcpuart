[working-directory: 'kernel_module']
build-module:
    make 

[working-directory: 'kernel_module']
insert-module: build-module
    -sudo rmmod tcpuart
    sudo insmod tcpuart.ko

log:
    sudo dmesg -w

[working-directory: 'test']
build-test:
    make

[working-directory: 'test']
run-test: build-test
    ./main
