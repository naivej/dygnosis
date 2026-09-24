// inventory: d_pac_e452_target_without_pac_model
var x;
pac_target_info(p);
  target x;
  auxname_target_nonstationary x_ns;
  component x;
  auxname x_part;
  kind dd;
end;
model;
  [name='X'] x=pac_target_nonstationary(p);
end;
