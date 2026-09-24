// inventory: p_pac_e001_deterministic_timing
var y;
model;
  y=0;
end;
deterministic_trends;
  y(y(-1));
end;
