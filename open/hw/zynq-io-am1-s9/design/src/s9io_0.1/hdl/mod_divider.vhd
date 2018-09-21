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
-- Description:    Modulo divider
--
-- Engineer:       Marian Pristach
-- Revision:       1.0.0 (18.08.2018)
-- Comments:
----------------------------------------------------------------------------------------------------
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity mod_divider is
    generic (
        W : natural := 16             -- width of counter
    );
    port (
        clk       : in  std_logic;
        rst       : in  std_logic;    -- asynchronous, active low
        clear     : in  std_logic;
        division  : in  std_logic_vector(W-1 downto 0);
        output_en : out std_logic     -- buffered enable signal, active for 1 clk period
    );
end mod_divider;

architecture rtl of mod_divider is

    -- counter
    signal cnt_d, cnt_q : unsigned(W-1 downto 0);

    -- buffered output signal
    signal output_en_q, output_en_d : std_logic;

begin

    ------------------------------------------------------------------------------------------------
    -- sequential part of counter
    process (clk) begin
        if rising_edge(clk) then
            if (rst = '0') then
                cnt_q <= (others => '0');
            else
                cnt_q <= cnt_d;
            end if;
        end if;
    end process;

    ------------------------------------------------------------------------------------------------
    -- combinational part of counter
    process (cnt_q, division, clear) begin
        cnt_d <= cnt_q + 1;
        output_en_d <= '0';
        if (clear = '1') then
            cnt_d <= (others => '0');
            output_en_d <= '0';
        elsif (cnt_q = unsigned(division)) then
            cnt_d <= (others => '0');
            output_en_d <= '1';
        end if;
    end process;

    ------------------------------------------------------------------------------------------------
    -- output buffer
    process (clk) begin
        if rising_edge(clk) then
            if (rst = '0') then
                output_en_q <= '0';
            else
                output_en_q <= output_en_d;
            end if;
        end if;
    end process;

    ------------------------------------------------------------------------------------------------
    -- output signals
    output_en <= output_en_q;

end rtl;
