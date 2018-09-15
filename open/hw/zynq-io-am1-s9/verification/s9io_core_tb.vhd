----------------------------------------------------------------------------------------------------
-- Company:        Braiins Systems s.r.o.
-- Engineer:       Marian Pristach
--
-- Project Name:   S9 Board Interface IP
-- Description:    Testbench for S9 Board Interface IP
--
-- Revision:       0.0.1 (18.08.2018)
-- Comments:
----------------------------------------------------------------------------------------------------
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity s9io_core_tb is
end s9io_core_tb;

architecture behavioral of s9io_core_tb is

	-- component declaration for the design under test (dut)

	component s9io_core
	port(
		clk : in  std_logic;
		rst : in  std_logic;
		rxd : in  std_logic;
		txd : out  std_logic;
		irq_work_tx : out std_logic;
		irq_work_rx : out std_logic;
		irq_cmd_rx : out std_logic;
		work_time_ack : out  std_logic;
		cmd_rx_fifo_rd : in  std_logic;
		cmd_rx_fifo_data : out  std_logic_vector(31 downto 0);
		cmd_tx_fifo_wr : in  std_logic;
		cmd_tx_fifo_data : in  std_logic_vector(31 downto 0);
		work_rx_fifo_rd : in  std_logic;
		work_rx_fifo_data : out  std_logic_vector(31 downto 0);
		work_tx_fifo_wr : in  std_logic;
		work_tx_fifo_data : in  std_logic_vector(31 downto 0);
		reg_ctrl : in  std_logic_vector(15 downto 0);
		reg_status : out  std_logic_vector(12 downto 0);
		reg_uart_divisor : in  std_logic_vector(11 downto 0);
		reg_work_time : in  std_logic_vector(23 downto 0);
		reg_irq_fifo_thr : in  std_logic_vector(10 downto 0);
		reg_err_counter : out  std_logic_vector(31 downto 0)
	);
	end component;


	--Inputs
	signal clk : std_logic := '0';
	signal rst : std_logic := '0';
	signal rxd : std_logic := '0';
	signal cmd_rx_fifo_rd : std_logic := '0';
	signal cmd_tx_fifo_wr : std_logic := '0';
	signal cmd_tx_fifo_data : std_logic_vector(31 downto 0) := (others => '0');
	signal work_rx_fifo_rd : std_logic := '0';
	signal work_tx_fifo_wr : std_logic := '0';
	signal work_tx_fifo_data : std_logic_vector(31 downto 0) := (others => '0');
	signal reg_ctrl : std_logic_vector(15 downto 0) := (others => '0');
	signal reg_uart_divisor : std_logic_vector(11 downto 0) := (others => '0');
	signal reg_work_time : std_logic_vector(23 downto 0) := (others => '0');
	signal reg_irq_fifo_thr : std_logic_vector(10 downto 0) := (others => '0');

	--Outputs
	signal txd : std_logic;
	signal irq_work_tx : std_logic;
	signal irq_work_rx : std_logic;
	signal irq_cmd_rx : std_logic;
	signal work_time_ack : std_logic;
	signal cmd_rx_fifo_data : std_logic_vector(31 downto 0);
	signal work_rx_fifo_data : std_logic_vector(31 downto 0);
	signal reg_status : std_logic_vector(12 downto 0);
	signal reg_err_counter : std_logic_vector(31 downto 0);

    ------------------------------------------------------------------------------------------------
	-- testbench UART signals
	-- FIFO read port
	signal uart_rx_read    : std_logic;
	signal uart_rx_empty   : std_logic;
	signal uart_rx_data_rd : std_logic_vector(7 downto 0);

	-- FIFO write port
	signal uart_tx_write   : std_logic;
	signal uart_tx_full    : std_logic;
	signal uart_tx_data_wr : std_logic_vector(7 downto 0);

	-- UART status
	signal uart_frame_err  : std_logic;
	signal uart_over_err   : std_logic;

    ------------------------------------------------------------------------------------------------
	-- Clock period definitions
	constant clk_period : time := 20 ns;

	-- simulation control
	signal end_of_sim : boolean := false;

    ------------------------------------------------------------------------------------------------
	-- FIFO and UART generic data type
	type fifo_t is array(natural range <>) of std_logic_vector(31 downto 0);
	type uart_t is array(natural range <>) of std_logic_vector(7 downto 0);

    ------------------------------------------------------------------------------------------------
	-- 1 midstate work
	constant work_fifo_1 : fifo_t(0 to 11) := (X"00000000", X"ffffffff", X"ffffffff", X"ffffffff", X"00000000", X"00000000", X"00000000", X"00000000", X"00000000", X"00000000", X"00000000", X"00000000");

	constant work_uart_1 : uart_t(0 to 53) := (X"21", X"36", X"00", X"01", X"00", X"00", X"00", X"00", X"ff", X"ff", X"ff", X"ff", X"ff", X"ff", X"ff", X"ff", X"ff", X"ff", X"ff", X"ff", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"00", X"5f", X"d3");

    ------------------------------------------------------------------------------------------------
	-- 4 midstates work
	constant work_fifo_4 : fifo_t(0 to 35) := (X"00000031", X"17365a17", X"5b51c8e6", X"66014b9d", X"1df9f7a3", X"ba9aca03", X"c42b0a8c", X"d89fc91a", X"1046e72e", X"46a47e9a", X"f01c1b8e", X"ebc3c539", X"e578935d", X"c6419d97", X"1ff8d327", X"7bf6698e", X"d757b9eb", X"980317d2", X"eafd359f", X"9544a768", X"0e1d09af", X"c9316c84", X"89bbde77", X"cb13866a", X"805beaaa", X"ffbbfdb1", X"a1b617a9", X"a81b497c", X"93c5272d", X"cd1b2770", X"96ab3905", X"7bfafae3", X"f1004cdb", X"b08d4078", X"d82c00af", X"e75b218b");

	constant work_uart_4 : uart_t(0 to 149) := (X"21", X"96", X"31", X"04", X"00", X"00", X"00", X"00", X"17", X"5a", X"36", X"17", X"e6", X"c8", X"51", X"5b", X"9d", X"4b", X"01", X"66", X"1d", X"f9", X"f7", X"a3", X"ba", X"9a", X"ca", X"03", X"c4", X"2b", X"0a", X"8c", X"d8", X"9f", X"c9", X"1a", X"10", X"46", X"e7", X"2e", X"46", X"a4", X"7e", X"9a", X"f0", X"1c", X"1b", X"8e", X"eb", X"c3", X"c5", X"39", X"e5", X"78", X"93", X"5d", X"c6", X"41", X"9d", X"97", X"1f", X"f8", X"d3", X"27", X"7b", X"f6", X"69", X"8e", X"d7", X"57", X"b9", X"eb", X"98", X"03", X"17", X"d2", X"ea", X"fd", X"35", X"9f", X"95", X"44", X"a7", X"68", X"0e", X"1d", X"09", X"af", X"c9", X"31", X"6c", X"84", X"89", X"bb", X"de", X"77", X"cb", X"13", X"86", X"6a", X"80", X"5b", X"ea", X"aa", X"ff", X"bb", X"fd", X"b1", X"a1", X"b6", X"17", X"a9", X"a8", X"1b", X"49", X"7c", X"93", X"c5", X"27", X"2d", X"cd", X"1b", X"27", X"70", X"96", X"ab", X"39", X"05", X"7b", X"fa", X"fa", X"e3", X"f1", X"00", X"4c", X"db", X"b0", X"8d", X"40", X"78", X"d8", X"2c", X"00", X"af", X"e7", X"5b", X"21", X"8b", X"37", X"0a");

    ------------------------------------------------------------------------------------------------
	-- check_asic_reg(CHIP_ADDRESS) -> read_asic_register(chain, 1, 0, CHIP_ADDRESS)
	-- software_set_address
	-- software_set_address -> 64x set_address(i, 0, chip_addr) // chip_addr = 0, 4, 8, 12
	constant cmd_fifo_5 : fifo_t(0 to 8) := (
		X"00000554",
		X"00000555",
		X"00000541",
		X"00040541",
		X"00080541",
		X"000c0541",
		X"00f40541",
		X"00f80541",
		X"00fc0541"
	);
	constant cmd_uart_5 : uart_t(0 to 44) := (
		X"54", X"05", X"00", X"00", X"19",
		X"55", X"05", X"00", X"00", X"10",
		X"41", X"05", X"00", X"00", X"15",
		X"41", X"05", X"04", X"00", X"0a",
		X"41", X"05", X"08", X"00", X"0e",
		X"41", X"05", X"0c", X"00", X"11",
		X"41", X"05", X"f4", X"00", X"10",
		X"41", X"05", X"f8", X"00", X"14",
		X"41", X"05", X"fc", X"00", X"0b"
	);

    ------------------------------------------------------------------------------------------------
	-- set_frequency(dev->frequency) -> set_frequency_with_addr_plldatai(pllindex, mode, addr, chain)
	-- open_core_one_chain(chainIndex, nullwork_enable) ->
	--   BC_COMMAND_BUFFER_READY | BC_COMMAND_EN_CHAIN_ID | (chainIndex << 16) | (bc_command & 0xfff0ffff)
	-- set_asic_ticket_mask(63)
	-- set_hcnt(0)
	constant cmd_fifo_9 : fifo_t(0 to 21) := (
		X"0c000948", X"21026800",
		X"0c040948", X"21026800",
		X"0c080948", X"21026800",
		X"0c0c0948", X"21026800",
		X"0ce80948", X"21026800",
		X"0cec0948", X"21026800",
		X"0cf00948", X"21026800",
		X"0cf40948", X"21026800",
		X"1c000958", X"809a2040",
		X"18000958", X"3f000000",
		X"14000958", X"00000000"
	);
	constant cmd_uart_9 : uart_t(0 to 98) := (
		X"48", X"09", X"00", X"0c", X"00", X"68", X"02", X"21", X"02",
		X"48", X"09", X"04", X"0c", X"00", X"68", X"02", X"21", X"19",
		X"48", X"09", X"08", X"0c", X"00", X"68", X"02", X"21", X"11",
		X"48", X"09", X"0c", X"0c", X"00", X"68", X"02", X"21", X"0a",
		X"48", X"09", X"e8", X"0c", X"00", X"68", X"02", X"21", X"03",
		X"48", X"09", X"ec", X"0c", X"00", X"68", X"02", X"21", X"18",
		X"48", X"09", X"f0", X"0c", X"00", X"68", X"02", X"21", X"13",
		X"48", X"09", X"f4", X"0c", X"00", X"68", X"02", X"21", X"08",
		X"58", X"09", X"00", X"1c", X"40", X"20", X"9a", X"80", X"00",
		X"58", X"09", X"00", X"18", X"00", X"00", X"00", X"3f", X"00",
		X"58", X"09", X"00", X"14", X"00", X"00", X"00", X"00", X"0a"
	);

	constant resp_fifo_7 : fifo_t(0 to 3) := (
		-- work response - the real value depends on the last work ID !
-- 		X"083c0648", X"99001200"
		X"083c0648", X"99FF9200",
		-- command response
		X"f4908713", X"1c000000"
	);
	constant resp_uart_7 : uart_t(0 to 13) := (
		-- work response
		X"48", X"06", X"3c", X"08", X"00", X"12", X"99",
		-- command response
		X"13", X"87", X"90", X"f4", X"00", X"00", X"1c"
	);


begin

	-- Instantiate the Design Under Test (DUT)
	dut: s9io_core
	port map (
		clk => clk,
		rst => rst,
		rxd => rxd,
		txd => txd,
		irq_work_tx => irq_work_tx,
		irq_work_rx => irq_work_rx,
		irq_cmd_rx => irq_cmd_rx,
		work_time_ack => work_time_ack,
		cmd_rx_fifo_rd => cmd_rx_fifo_rd,
		cmd_rx_fifo_data => cmd_rx_fifo_data,
		cmd_tx_fifo_wr => cmd_tx_fifo_wr,
		cmd_tx_fifo_data => cmd_tx_fifo_data,
		work_rx_fifo_rd => work_rx_fifo_rd,
		work_rx_fifo_data => work_rx_fifo_data,
		work_tx_fifo_wr => work_tx_fifo_wr,
		work_tx_fifo_data => work_tx_fifo_data,
		reg_ctrl => reg_ctrl,
		reg_status => reg_status,
		reg_uart_divisor => reg_uart_divisor,
		reg_work_time => reg_work_time,
		reg_irq_fifo_thr => reg_irq_fifo_thr,
		reg_err_counter => reg_err_counter
	);

	--------------------------------------------------------------------------------
	-- reset design
	rst <= '0', '1' after clk_period*2;

	-- clock generation
	p_clk_gen: process
	begin
		if end_of_sim = false then
			clk <= '0'; wait for clk_period/2;
			clk <= '1'; wait for clk_period/2;
		else
			clk <= '0'; wait;
		end if;
	end process;


	-- Stimulus process
	stim_proc: process
	begin
		cmd_rx_fifo_rd <= '0';
		cmd_tx_fifo_wr <= '0';
		cmd_tx_fifo_data <= (others => '0');

		work_rx_fifo_rd <= '0';
		work_tx_fifo_wr <= '0';
		work_tx_fifo_data <= (others => '0');

		reg_ctrl <= (others => '0');
		reg_uart_divisor <= (others => '0');
		reg_work_time <= (others => '0');
		reg_irq_fifo_thr <= (others => '0');

        -- UART interface
        uart_rx_read    <= '0';
        uart_tx_write   <= '0';
        uart_tx_data_wr <= (others => '0');

		wait until rising_edge(rst);
		wait until falling_edge(clk);

		-- enable IP core
		reg_ctrl <= X"8000";    -- 1 midstate mode
		reg_uart_divisor <= std_logic_vector(to_unsigned(0, 12));  -- @50MHz -> 3.125MBd
		reg_work_time <= std_logic_vector(to_unsigned(500, 24));  -- @50MHz -> 10us
		reg_irq_fifo_thr <= std_logic_vector(to_unsigned(10, 11));  -- for test  only

		wait for clk_period;


-- 		for i in cmd_fifo_5'range loop
-- 			cmd_tx_fifo_wr <= '1';
-- 			cmd_tx_fifo_data <= cmd_fifo_5(i);
-- 			wait for clk_period;
-- 		end loop;
--
-- 		cmd_tx_fifo_wr <= '0';
-- 		cmd_tx_fifo_data <= (others => '0');
-- 		wait for 200us;

-- 		for i in cmd_fifo_9'range loop
-- 			cmd_tx_fifo_wr <= '1';
-- 			cmd_tx_fifo_data <= cmd_fifo_9(i);
-- 			wait for clk_period;
-- 		end loop;
--
-- 		cmd_tx_fifo_wr <= '0';
-- 		cmd_tx_fifo_data <= (others => '0');
-- 		wait for 400us;



		for i in cmd_fifo_5'range loop
			cmd_tx_fifo_wr <= '1';
			cmd_tx_fifo_data <= cmd_fifo_5(i);
			wait for clk_period;
			cmd_tx_fifo_wr <= '0';
			wait for clk_period*100;
			exit when i = 2;
		end loop;
		cmd_tx_fifo_data <= (others => '0');

		-- reset IP core
		wait for clk_period;
		reg_ctrl <= X"0000";    -- disable IP core
		wait for clk_period;
		reg_ctrl <= X"8000";    -- enable IP core, 1 midstate mode
		wait for clk_period;

		for i in cmd_fifo_5'range loop
			cmd_tx_fifo_wr <= '1';
			cmd_tx_fifo_data <= cmd_fifo_5(i);
			wait for clk_period;
		end loop;
		cmd_tx_fifo_wr <= '0';
		cmd_tx_fifo_data <= (others => '0');

		wait for 200us;



		for i in work_fifo_1'range loop
			work_tx_fifo_wr <= '1';
			work_tx_fifo_data <= work_fifo_1(i);
			wait for clk_period;
			work_tx_fifo_wr <= '0';
			wait for clk_period*100;
			exit when i = 2;
		end loop;
		work_tx_fifo_data <= (others => '0');

		-- reset IP core
		wait for clk_period;
		reg_ctrl <= X"0000";    -- disable IP core
		wait for clk_period;
		reg_ctrl <= X"8000";    -- enable IP core, 1 midstate mode
		wait for clk_period;

		for i in work_fifo_1'range loop
			work_tx_fifo_wr <= '1';
			work_tx_fifo_data <= work_fifo_1(i);
			wait for clk_period;
		end loop;
		work_tx_fifo_wr <= '0';
		work_tx_fifo_data <= (others => '0');

		wait for 200us;





-- 		reg_ctrl <= X"C800";    -- 4 midstates mode, TX IRQ enabled

-- 		for i in work_fifo_4'range loop
-- 			work_tx_fifo_wr <= '1';
-- 			work_tx_fifo_data <= work_fifo_4(i);
-- 			wait for clk_period;
-- 		end loop;
--
-- 		work_tx_fifo_wr <= '0';
-- 		work_tx_fifo_data <= (others => '0');
-- 		wait for 100us;

-- 		for i in resp_uart_7'range loop
-- 			uart_tx_write <= '1';
-- 			uart_tx_data_wr <= resp_uart_7(i);
-- 			wait for clk_period;
-- 		end loop;
--
--         uart_tx_write   <= '0';
--         uart_tx_data_wr <= (others => '0');
-- 		wait for 100us;

		end_of_sim <= true;
		wait;

-- 		cmd_rx_fifo_rd : in  std_logic;
-- 		work_rx_fifo_rd : in  std_logic;

	end process;


	------------------------------------------------------------------------------------------------
	i_tb_uart: entity work.uart
	port map (
		clk        => clk,
		rst        => rst,

		-- UART interface
		rxd        => txd,
		txd        => rxd,

		-- synchronous clear FIFOs
		clear      => '0',

		-- FIFO read port
		rx_read    => uart_rx_read,
		rx_empty   => uart_rx_empty,
		rx_data_rd => uart_rx_data_rd,

		-- FIFO write port
		tx_write   => uart_tx_write,
		tx_full    => uart_tx_full,
		tx_data_wr => uart_tx_data_wr,

		-- UART configuration
		division   => reg_uart_divisor,

		-- UART status
		frame_err  => uart_frame_err,
		over_err   => uart_over_err
	);

end behavioral;
