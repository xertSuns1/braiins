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
-- Description:    UART Interface Receiver Module
--
-- Engineer:       Marian Pristach
-- Revision:       1.0.0 (18.08.2018)
-- Comments:
----------------------------------------------------------------------------------------------------
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity uart_rx is
    generic (
        DB  : integer := 8;    -- number of data bits
        SPB : integer := 16    -- number of samples per one bit
    );
    port (
        clk       : in  std_logic;
        rst       : in  std_logic;

        -- clock enable signal from baud rate generator
        clk_en    : in  std_logic;

        -- UART interface
        rxd       : in  std_logic;

        -- internal interface
        rx_done   : out std_logic;
        rx_data   : out std_logic_vector(DB-1 downto 0);
        frame_err : out std_logic
    );
end uart_rx ;

architecture rtl of uart_rx is

    -- bits counter, size depends on number of data bits
    signal bits_cnt_q    : unsigned(2 downto 0);
    signal bits_cnt_d    : unsigned(2 downto 0);

    -- samples counter, size depends on number of samples per one bit and number of stop bits
    signal samples_cnt_q : unsigned(3 downto 0);
    signal samples_cnt_d : unsigned(3 downto 0);

    -- data shift register, size depends on number of data bits
    signal data_q        : std_logic_vector(DB-1 downto 0);
    signal data_d        : std_logic_vector(DB-1 downto 0);

    -- FSM type and signals declaration
    type fsm_type_t is (st_idle, st_start, st_data, st_stop);
    signal fsm_d         : fsm_type_t;
    signal fsm_q         : fsm_type_t;

begin

    ------------------------------------------------------------------------------------------------
    -- sequential part of FSM (state register)
    p_fsm_seq: process (clk) begin
        if rising_edge(clk) then
            if (rst = '0') then
                fsm_q <= st_idle;
                bits_cnt_q <= (others => '0');
                samples_cnt_q <= (others => '0');
                data_q <= (others => '0');
            else
                fsm_q <= fsm_d;
                bits_cnt_q <= bits_cnt_d;
                samples_cnt_q <= samples_cnt_d;
                data_q <= data_d;
            end if;
        end if;
    end process;

    -- combinational part of FSM (next-state logic)
    p_fsm_cmb: process (fsm_q, bits_cnt_q, samples_cnt_q, data_q, rxd, clk_en) begin

        -- default assignment to registers and signals
        fsm_d <= fsm_q;
        bits_cnt_d <= bits_cnt_q;
        samples_cnt_d <= samples_cnt_q;
        data_d <= data_q;
        rx_done <= '0';
        frame_err <= '0';

        -- state machine
        case fsm_q is
            when st_idle =>
                if (rxd = '0') then
                    fsm_d <= st_start;
                    samples_cnt_d <= (others => '0');
                    bits_cnt_d <= (others => '0');
                end if;

            when st_start =>
                if (clk_en = '1') then
                    if (samples_cnt_q = ((SPB/2)-1)) then
                        samples_cnt_d <= (others => '0');
                        if (rxd = '0') then    -- test value of half start bit
                            fsm_d <= st_data;
                        else
                            fsm_d <= st_idle;   -- return to idle if false-positive start is detected
                        end if;
                    else
                        samples_cnt_d <= samples_cnt_q + 1;
                    end if;
                end if;

            when st_data =>
                if (clk_en = '1') then
                    if (samples_cnt_q = (SPB-1)) then
                        samples_cnt_d <= (others => '0');
                        data_d <= rxd & data_q(DB-1 downto 1);
                        if (bits_cnt_q = (DB-1)) then
                            fsm_d <= st_stop;
                        else
                            bits_cnt_d <= bits_cnt_q + 1;
                        end if;
                    else
                        samples_cnt_d <= samples_cnt_q + 1;
                    end if;
                end if;

            when st_stop =>
                if (clk_en = '1') then
                    if (samples_cnt_q = (SPB-1)) then
                        fsm_d <= st_idle;
                        rx_done <= rxd;           -- done is set only if stop bit is one
                        frame_err <= not(rxd);    -- set frame error if stop bit is zero
                    else
                        samples_cnt_d <= samples_cnt_q + 1;
                    end if;
                end if;
        end case;
    end process;

    ------------------------------------------------------------------------------------------------
    -- output signals
    rx_data <= data_q;

end rtl;
