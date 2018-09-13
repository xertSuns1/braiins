----------------------------------------------------------------------------------------------------
-- Company:        Braiins Systems s.r.o.
-- Engineer:       Marian Pristach
--
-- Project Name:   S9 Board Interface IP
-- Description:    Modulo divider
--
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
