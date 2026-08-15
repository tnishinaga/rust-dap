MEMORY {
    /* Pico 2 has a 4 MiB external flash; 2 MiB is safe for RP2350 boards. */
    FLASH : ORIGIN = 0x10000000, LENGTH = 2048K
    /* RP2350 SRAM0-SRAM7, striped mapping. */
    RAM : ORIGIN = 0x20000000, LENGTH = 512K
    /* Direct-mapped banks available for predictable data placement. */
    SRAM8 : ORIGIN = 0x20080000, LENGTH = 4K
    SRAM9 : ORIGIN = 0x20081000, LENGTH = 4K
}

SECTIONS {
    .start_block : ALIGN(4)
    {
        __start_block_addr = .;
        KEEP(*(.start_block));
        KEEP(*(.boot_info));
    } > FLASH
} INSERT AFTER .vector_table;

_stext = ADDR(.start_block) + SIZEOF(.start_block);
