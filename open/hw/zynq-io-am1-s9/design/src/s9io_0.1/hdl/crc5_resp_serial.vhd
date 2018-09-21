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
-- Description:    CRC-5 with Polynomial 0xD (modified), Serial Calculation per Bits
--
-- Engineer:       Marian Pristach
-- Revision:       1.0.0 (13.09.2018)
-- Comments:       Initial value defined by generic, no final xor
--                 MSB first of data_in, CRC results in direct order
----------------------------------------------------------------------------------------------------
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity crc5_resp_serial is
    generic (
        INIT    : std_logic_vector(4 downto 0)          -- initial value of calculation
    );
    port (
        clk     : in  std_logic;
        rst     : in  std_logic;                        -- synchronous, active low
        clear   : in  std_logic;                        -- synchronous clear
        data_wr : in  std_logic;                        -- write enable
        data_in : in  std_logic_vector(7 downto 0);     -- input data
        ready   : out std_logic;                        -- crc engine is ready
        crc     : out std_logic_vector(4 downto 0)      -- crc-5 output
    );
end crc5_resp_serial;


architecture rtl of crc5_resp_serial is

    -- FSM type and signals declaration
    type fsm_type_t is (st_idle, st_calc);
    signal fsm_d        : fsm_type_t;
    signal fsm_q        : fsm_type_t;

    -- data shift register
    signal data_shift_d : std_logic_vector(7 downto 0);
    signal data_shift_q : std_logic_vector(7 downto 0);

    -- shift counter
    signal cnt_d        : unsigned(2 downto 0);
    signal cnt_q        : unsigned(2 downto 0);

    -- clock enable for CRC logic
    signal clk_en       : std_logic;

    -- input data xor MSB of crc_reg
    signal crc_in       : std_logic;

    -- internal register for calculation
    signal crc_reg      : std_logic_vector(4 downto 0);

begin

    ------------------------------------------------------------------------------------------------
    -- sequential part of FSM (state register)
    p_fsm_seq: process (clk) begin
        if rising_edge(clk) then
            if (rst = '0') then
                fsm_q <= st_idle;
                cnt_q <= (others => '0');
                data_shift_q <= (others => '0');
            else
                fsm_q <= fsm_d;
                cnt_q <= cnt_d;
                data_shift_q <= data_shift_d;
            end if;
        end if;
    end process;

    ------------------------------------------------------------------------------------------------
    -- combinational part of transmit FSM (next-state logic)
    p_fsm_cmb: process (fsm_q, cnt_q, data_shift_q, data_wr, data_in) begin

        -- default assignment to registers and signals
        fsm_d <= fsm_q;
        cnt_d <= cnt_q;
        data_shift_d <= data_shift_q;
        ready <= '0';
        clk_en <= '0';

        -- state machine
        case fsm_q is
            when st_idle =>
                ready <= '1';
                if (data_wr = '1') then
                    fsm_d <= st_calc;
                    cnt_d <= "111";
                    data_shift_d <= data_in;
                end if;

            when st_calc =>
                data_shift_d <= data_shift_q(6 downto 0) & '0';
                cnt_d <= cnt_q - 1;
                clk_en <= '1';
                if (cnt_q = "000") then
                    fsm_d <= st_idle;
                end if;

        end case;
    end process;


    ----------------------------------------------------------------------------------
    crc_in <= data_shift_q(7) xor crc_reg(4);

    ----------------------------------------------------------------------------------
    -- sequential part
    process (clk) begin
        if rising_edge(clk) then
            if (rst = '0') then
                crc_reg <= INIT;
            elsif (clear = '1') then
                crc_reg <= INIT;
            elsif (clk_en = '1') then
                crc_reg <= crc_reg(3 downto 0) & crc_in;        -- universal shift
                crc_reg(2) <= crc_reg(1) xor crc_in;            -- update bit based on crc polynom
                crc_reg(3) <= crc_reg(2) xor data_shift_q(7);   -- non-standard update
            end if;
        end if;
    end process;

    ----------------------------------------------------------------------------------
    -- direct order
    crc <= crc_reg;

end rtl;

