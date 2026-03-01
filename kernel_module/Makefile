obj-m += tcpuart.o
tcpuart-y := src/tcpuart.o src/message.o
ccflags-y := -I$(src)/include

KDIR := /lib/modules/$(shell uname -r)/build

all:
	make -C $(KDIR) M=$(PWD) modules

clean:
	make -C $(KDIR) M=$(PWD) clean
