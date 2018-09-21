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
-- Description:    FIFO buffer with synchronous read using block memory
--
-- Engineer:       Marian Pristach
-- Revision:       1.0.0 (18.08.2018)
-- Comments:
----------------------------------------------------------------------------------------------------
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity fifo_block is
    generic(
        A : natural := 5;           -- number of address bits
        W : natural := 8            -- number of data bits
    );
    port (
        clk    : in  std_logic;
        rst    : in  std_logic;

        -- synchronous clear of FIFO
        clear  : in std_logic;

        -- write port
        wr     : in  std_logic;
        full   : out std_logic;
        data_w : in  std_logic_vector(W-1 downto 0);

        -- read port
        rd     : in  std_logic;
        empty  : out std_logic;
        data_r : out std_logic_vector(W-1 downto 0)
    );
end fifo_block;

architecture rtl of fifo_block is

    -- definition of memory type
    type ram_t is array(0 to (2**A)-1) of std_logic_vector(W-1 downto 0);
    signal ram       : ram_t;

    -- internal read/write signals
    signal rd_en     : std_logic;
    signal wr_en     : std_logic;

    -- FIFO flags
    signal full_d    : std_logic;
    signal full_q    : std_logic;
    signal empty_d   : std_logic;
    signal empty_q   : std_logic;

    -- read pointer and signal with incremented value
    signal w_ptr_d   : unsigned(A-1 downto 0);
    signal w_ptr_q   : unsigned(A-1 downto 0);
    signal w_ptr_inc : unsigned(A-1 downto 0);

    -- write pointer and signal with incremented value
    signal r_ptr_d   : unsigned(A-1 downto 0);
    signal r_ptr_q   : unsigned(A-1 downto 0);
    signal r_ptr_inc : unsigned(A-1 downto 0);

    -- type of operation of the FIFO
    signal wr_op     : std_logic_vector(1 downto 0);

    -- local signals for read data
    signal data_r_d  : std_logic_vector(W-1 downto 0);

begin

    ------------------------------------------------------------------------------------------------
    -- read enabled only when FIFO is not empty
    rd_en <= rd and (not empty_q);

    -- write enabled only when FIFO is not full
    wr_en <= wr and (not full_q);

    ------------------------------------------------------------------------------------------------
    -- access to memory - write port
    p_write: process (clk) begin
        if rising_edge(clk) then
            if (wr_en = '1') then
                ram(to_integer(w_ptr_q)) <= data_w;
            end if;
        end if;
    end process;

    -- access to memory - synchronous read port
    p_read: process (clk) begin
        if rising_edge(clk) then
            if (rd_en = '1') then
                data_r_d <= ram(to_integer(r_ptr_q));
            end if;
        end if;
    end process;

    ------------------------------------------------------------------------------------------------
    -- incremented pointer values
    w_ptr_inc <= w_ptr_q + 1;
    r_ptr_inc <= r_ptr_q + 1;

    ------------------------------------------------------------------------------------------------
    -- registers for read/write pointers and flags
    p_regs: process (clk) begin
        if rising_edge(clk) then
            if (rst = '0') then
                r_ptr_q <= (others => '0');
                w_ptr_q <= (others => '0');
                empty_q <= '1';
                full_q <= '0';
            else
                r_ptr_q <= r_ptr_d;
                w_ptr_q <= w_ptr_d;
                empty_q <= empty_d;
                full_q <= full_d;
            end if;
        end if;
    end process;

    -- type of operation
    wr_op <= wr_en & rd_en;

    ------------------------------------------------------------------------------------------------
    -- control logic
    p_comb: process (r_ptr_q, w_ptr_q, empty_q, full_q, wr_op, r_ptr_inc, w_ptr_inc, clear) begin
        r_ptr_d <= r_ptr_q;
        w_ptr_d <= w_ptr_q;
        empty_d <= empty_q;
        full_d <= full_q;

        case wr_op is
            when "01" =>
                r_ptr_d <= r_ptr_inc;
                full_d <= '0';
                if (r_ptr_inc = w_ptr_q) then
                    empty_d <= '1';
                end if;

            when "10" =>
                w_ptr_d <= w_ptr_inc;
                empty_d <= '0';
                if (w_ptr_inc = r_ptr_q) then
                    full_d <= '1';
                end if;

            when "11" =>
                r_ptr_d <= r_ptr_inc;
                w_ptr_d <= w_ptr_inc;

            when others =>
        end case;

        -- synchronous clear of FIFO with higher priority then read/write operations
        if (clear = '1') then
            r_ptr_d <= (others => '0');
            w_ptr_d <= (others => '0');
            empty_d <= '1';
            full_d <= '0';
        end if;

    end process;

    ------------------------------------------------------------------------------------------------
    -- output signals
    full <= full_q;
    empty <= empty_q;
    data_r <= data_r_d;

end rtl;
