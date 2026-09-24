// inventory: p_pac_e310_deterministic_trend
var y;
trend_var(growth_factor=1.01) A;
model;
  y=0;
end;
deterministic_trends;
  y(A);
end;
