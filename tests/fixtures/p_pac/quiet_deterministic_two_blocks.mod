// inventory: p_pac_quiet_deterministic_two_blocks
var x;
parameters beta;
beta = .9;
model;
  x = x(-1);
end;

deterministic_trends;
  x (beta);
end;
deterministic_trends;
  x (beta+1);
end;
