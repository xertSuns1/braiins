----------------------------------------------------------------------------------------------------
-- Copyright (c) 2018 Braiins Systems s.r.o.
--
-- Permission is hereby granted, free of charge, to any person obtaining a copy
-- of this software and associated documentation files (the "Software"), to deal
-- in the Software without restriction, including without limitation the rights
-- to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
-- copies of the Software, and to permit persons to whom the Software is
-- furnished to do so, subject to the following conditions:
--
-- The above copyright notice and this permission notice shall be included in all
-- copies or substantial portions of the Software.
--
-- THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
-- IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
-- FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
-- AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
-- LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
-- OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
-- SOFTWARE.
----------------------------------------------------------------------------------------------------
-- Project Name:   S9 Board Interface IP
-- Description:    UART Interface Module
--
-- Engineer:       Marian Pristach
-- Revision:       1.0.0 (18.08.2018)
-- Comments:
----------------------------------------------------------------------------------------------------
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity uart is
    port (
        clk        : in  std_logic;
        rst        : in  std_logic;

        -- UART interface
        rxd        : in  std_logic;
        txd        : out std_logic;

        -- synchronous clear FIFOs
        clear      : in  std_logic;

        -- FIFO read port
        rx_read    : in  std_logic;
        rx_empty   : out std_logic;
        rx_data_rd : out std_logic_vector(7 downto 0);

        -- FIFO write port
        tx_write   : in  std_logic;
        tx_full    : out std_logic;
        tx_data_wr : in  std_logic_vector(7 downto 0);

        -- UART configuration
        division   : in  std_logic_vector(11 downto 0);

        -- UART status
        sync_busy  : out std_logic;    -- baudrate synchronization busy signal
        frame_err  : out std_logic;
        over_err   : out std_logic
    );
end entity uart;

architecture RTL of uart is

    ------------------------------------------------------------------------------------------------
    -- UART configurations
    constant DB                    : natural := 8;        -- number of data bits
    constant SPB                   : natural := 16;       -- number of samples per one bit
    constant FIFO_ADDR_WIDTH       : natural := 4;        -- 16 words
    constant MOD_DIVISON_WIDTH     : natural := 12;       -- define width of division register

    ------------------------------------------------------------------------------------------------
    -- synchronization RxD into clk clock domain
    signal rxd_q0                   : std_logic_vector(2 downto 0);

    ------------------------------------------------------------------------------------------------
    -- UART specific registers, flags and signals
    signal frame_err_q, frame_err_d : std_logic;
    signal over_err_q, over_err_d   : std_logic;
    signal division_q               : std_logic_vector(MOD_DIVISON_WIDTH-1 downto 0);

    -- clock enable signal for UART
    signal clk_en_uart              : std_logic;

    -- Receiver FIFO signals
    signal rx_fifo_wr               : std_logic;
    signal rx_fifo_full             : std_logic;
    signal rx_fifo_data_w           : std_logic_vector(7 downto 0);
    signal rx_fifo_rd               : std_logic;
    signal rx_fifo_empty            : std_logic;
    signal rx_fifo_data_r           : std_logic_vector(7 downto 0);

    -- Transmitter FIFO signals
    signal tx_fifo_wr               : std_logic;
    signal tx_fifo_full             : std_logic;
    signal tx_fifo_data_w           : std_logic_vector(7 downto 0);
    signal tx_fifo_rd               : std_logic;
    signal tx_fifo_empty            : std_logic;
    signal tx_fifo_data_r           : std_logic_vector(7 downto 0);

    -- UART signals
    signal rx_frame_error           : std_logic;
    signal tx_ready                 : std_logic;

    -- UART TX ready signal
    signal uart_tx_ready            : std_logic;

begin

    ------------------------------------------------------------------------------------------------
    -- synchronization RxD into clk clock domain
    process (clk) begin
        if rising_edge(clk) then
            if (rst = '0') then
                rxd_q0 <= (others => '1');
            else
                rxd_q0 <= rxd & rxd_q0(2 downto 1);
            end if;
        end if;
    end process;

    ------------------------------------------------------------------------------------------------
    -- peripheral registers
    process (clk) begin
        if rising_edge(clk) then
            if (rst = '0') then
                frame_err_q <= '0';
                over_err_q  <= '0';
            else
                frame_err_q <= frame_err_d;
                over_err_q  <= over_err_d;
            end if;
        end if;
    end process;

    -- update baudrate only if transmitter is ready
    process (clk) begin
        if rising_edge(clk) then
            if (rst = '0') then
                division_q  <= (others => '0');
            elsif (uart_tx_ready = '1') then
                division_q  <= division;
            end if;
        end if;
    end process;

    ------------------------------------------------------------------------------------------------
    -- next state logic for peripheral registers
    process (
        frame_err_q, over_err_q,
        rx_fifo_full, rx_fifo_wr, rx_frame_error
    )
    begin
        frame_err_d   <= frame_err_q;
        over_err_d    <= over_err_q;

        -- check of FIFO buffer overflow
        if (rx_fifo_wr = '1') and (rx_fifo_full = '1') then
            over_err_d <= '1';
        end if;

        -- check of frame error
        if (rx_frame_error = '1') then
            frame_err_d <= '1';
        end if;
    end process;

    ------------------------------------------------------------------------------------------------
    -- UART TX is ready when transmitter is ready and no data in buffer
    uart_tx_ready <= tx_ready and tx_fifo_empty;

    ------------------------------------------------------------------------------------------------
    -- Modulo divider as baud-rate generator
    i_baud_rate_unit: entity work.mod_divider
    generic map (
        W => MOD_DIVISON_WIDTH   -- width of counter
    )
    port map (
        clk       => clk,
        rst       => rst,
        clear     => '0',
        division  => division_q,
        output_en => clk_en_uart
    );

    ------------------------------------------------------------------------------------------------
    -- Receiver FIFO
    i_rxd_fifo: entity work.fifo_distr
    generic map (
        A => FIFO_ADDR_WIDTH,    -- address width of FIFO
        W => 8                   -- number of data bits
    )
    port map (
        clk    => clk,
        rst    => rst,
        clear  => clear,
        -- write port
        wr     => rx_fifo_wr,
        full   => rx_fifo_full,
        data_w => rx_fifo_data_w,
        -- read port
        rd     => rx_fifo_rd,
        empty  => rx_fifo_empty,
        data_r => rx_fifo_data_r
    );

    -- Transmitter FIFO
    i_txd_fifo: entity work.fifo_distr
    generic map (
        A => FIFO_ADDR_WIDTH,    -- address width of FIFO
        W => 8                   -- number of data bits
    )
    port map (
        clk    => clk,
        rst    => rst,
        clear  => clear,
        -- write port
        wr     => tx_fifo_wr,
        full   => tx_fifo_full,
        data_w => tx_fifo_data_w,
        -- read port
        rd     => tx_fifo_rd,
        empty  => tx_fifo_empty,
        data_r => tx_fifo_data_r
    );

    tx_fifo_rd <= tx_ready and not(tx_fifo_empty) and clk_en_uart;

    ------------------------------------------------------------------------------------------------
    -- UART receiver unit
    i_uart_rxd_unit: entity work.uart_rx
    generic map (
        DB  => DB,        -- number of data bits
        SPB => SPB        -- number of samples per one bit
    )
    port map (
        clk       => clk,
        rst       => rst,
        -- clock enable signal from baud rate generator
        clk_en    => clk_en_uart,
        -- UART interfacce
        rxd       => rxd_q0(0),
        -- parallel output
        rx_done   => rx_fifo_wr,
        rx_data   => rx_fifo_data_w,
        frame_err => rx_frame_error
    );

    -- UART transmitter unit
    i_uart_txd_unit: entity work.uart_tx
    generic map (
        DB  => DB,        -- number of data bits
        SPB => SPB        -- number of samples per one bit
    )
    port map (
        clk      => clk,
        rst      => rst,
        -- clock enable signal from baud rate generator
        clk_en   => clk_en_uart,
        -- UART interfacce
        txd      => txd,
        -- parallel input data and transfer acknowledgment
        tx_start => tx_fifo_rd,
        tx_ready => tx_ready,
        tx_data  => tx_fifo_data_r,
        tx_done  => open
    );

    ------------------------------------------------------------------------------------------------
    -- Output signals

    -- FIFO read port
    rx_fifo_rd     <= rx_read;
    rx_empty       <= rx_fifo_empty;
    rx_data_rd     <= rx_fifo_data_r;

    -- FIFO write port
    tx_fifo_wr     <= tx_write;
    tx_full        <= tx_fifo_full;
    tx_fifo_data_w <= tx_data_wr;

    -- UART status
    sync_busy <= '1' when ((uart_tx_ready = '0') and (division_q /= division)) else '0';
    frame_err <= frame_err_q;
    over_err  <= over_err_q;


end architecture;


