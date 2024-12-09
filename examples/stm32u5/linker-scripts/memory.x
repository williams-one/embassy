MEMORY
{
  FLASH        : ORIGIN = 0x08000000, LENGTH = 4096K - 256K /* BANK_1 + BANK_2 */
  USER_STORAGE : ORIGIN = ORIGIN(FLASH) + LENGTH(FLASH), LENGTH = 256K
  EXT_FLASH    : ORIGIN = 0xA0000000, LENGTH = 128M
  RAM          : ORIGIN = 0x20000000, LENGTH = 3008K /* SRAM + SRAM2 + SRAM3 + SRAM5 + SRAM6 */
}

__user_storage_start = ORIGIN(USER_STORAGE) - ORIGIN(FLASH);

SECTIONS
{
  ExtFlashSection :
  {
    *(.ExtFlashSection .ExtFlashSection.*)
    *(.gnu.linkonce.r.*)
    . = ALIGN(0x4);
  } >EXT_FLASH
}
