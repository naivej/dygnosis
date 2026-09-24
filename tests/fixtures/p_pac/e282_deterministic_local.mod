// inventory: p_pac_e282_deterministic_local
var y;
model;
  #loc=0.5;
  y=loc;
end;
deterministic_trends;
  y(loc);
end;
