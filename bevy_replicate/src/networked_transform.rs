use crate::{client::NetworkInterpolation, network_frame::NetworkedComponent};

use bevy::{ecs::world::EntityMut, prelude::*};
use bit_serializer::{BitReader, BitWriter};
use std::f32::consts::FRAC_1_SQRT_2;
use std::io;

// TODO: add configuration
pub struct TransformNetworked;

#[derive(Debug, Component)]
pub struct InterpolateTransform {
    from: Transform,
    to: Transform,
}

impl NetworkedComponent for TransformNetworked {
    type Component = Transform;

    fn can_delta(_old: &Self::Component, _new: &Self::Component) -> bool {
        true
    }

    fn write_delta(_old: &Self::Component, new: &Self::Component, writer: &mut BitWriter) -> Result<(), io::Error> {
        // TODO: still need to impl the delta
        Self::write_full(new, writer)
    }

    fn read_delta(_old: &Self::Component, reader: &mut BitReader) -> Result<Self::Component, io::Error> {
        // TODO: still need to impl the delta
        Self::read_full(reader)
    }

    fn write_full(transform: &Transform, writer: &mut BitWriter) -> Result<(), io::Error> {
        let translation = transform.translation;
        write_f32_range(writer, translation.x, -256.0, 255.0, 0.01)?;
        write_f32_range(writer, translation.y, 0.0, 32.0, 0.01)?;
        write_f32_range(writer, translation.z, -256.0, 255.0, 0.01)?;

        let rotation = transform.rotation;
        write_quat(writer, rotation, 11)?;

        let scale = transform.scale;
        write_f32_range(writer, scale.x, 0.0, 128.0, 0.01)?;
        write_f32_range(writer, scale.y, 0.0, 128.0, 0.01)?;
        write_f32_range(writer, scale.z, 0.0, 128.0, 0.01)?;

        Ok(())
    }

    fn read_full(reader: &mut BitReader) -> Result<Self::Component, io::Error> {
        let t_x = read_f32_range(reader, -256.0, 255.0, 0.01)?;
        let t_y = read_f32_range(reader, 0.0, 32.0, 0.01)?;
        let t_z = read_f32_range(reader, -256.0, 255.0, 0.01)?;

        let translation = Vec3::new(t_x, t_y, t_z);
        // println!("translation: {:?}", translation);

        let rotation = read_quat(reader, 11)?;
        // println!("rotation: {:?}", rotation);

        let s_x = read_f32_range(reader, 0.0, 128.0, 0.01)?;
        let s_y = read_f32_range(reader, 0.0, 128.0, 0.01)?;
        let s_z = read_f32_range(reader, 0.0, 128.0, 0.01)?;
        let scale = Vec3::new(s_x, s_y, s_z);
        // println!("scale: {:?}", scale);

        Ok(Transform {
            translation,
            rotation,
            scale,
        })
    }

    fn apply(mut entity_mut: EntityMut<'_>, component: &Self::Component) {
        let from = match entity_mut.get::<Transform>() {
            Some(t) => *t,
            None => {
                entity_mut.insert(*component);
                *component
            }
        };

        entity_mut.insert(InterpolateTransform { from, to: *component });
    }
}

pub fn interpolate_transform_system(interpolation: Res<NetworkInterpolation>, mut query: Query<(&mut Transform, &InterpolateTransform)>) {
    let t = interpolation.0;
    for (mut transform, interpolate) in query.iter_mut() {
        transform.translation = interpolate.from.translation.lerp(interpolate.to.translation, t);
        transform.scale = interpolate.from.scale.lerp(interpolate.to.scale, t);
        transform.rotation = interpolate.from.rotation.slerp(interpolate.to.rotation, t);
    }
}

fn bits_required(min: u32, max: u32) -> usize {
    let diff = max - min;
    (u32::BITS - diff.leading_zeros()) as usize
}

fn write_f32_range(writer: &mut BitWriter, value: f32, min: f32, max: f32, precision: f32) -> Result<(), io::Error> {
    let delta = max - min;
    let values = delta / precision;

    let max_integer_value = values.ceil() as u32;
    let bits = bits_required(0, max_integer_value);

    let normalized_value = ((value - min) / delta).clamp(0., 1.);
    let integer_value = (normalized_value * max_integer_value as f32 + 0.5).floor() as u32;

    writer.write_bits(integer_value, bits)?;

    Ok(())
}

fn read_f32_range(reader: &mut BitReader, min: f32, max: f32, precision: f32) -> Result<f32, io::Error> {
    let delta = max - min;
    let values = delta / precision;

    let max_integer_value = values.ceil() as u32;
    let bits = bits_required(0, max_integer_value);

    let integer_value = reader.read_bits(bits)?;

    let normalized_value = integer_value as f32 / max_integer_value as f32;
    let value = normalized_value * delta + min;

    Ok(value)
}

fn write_f32_range_bits(writer: &mut BitWriter, mut value: f32, min: f32, max: f32, bits: usize) -> Result<(), io::Error> {
    let delta = max - min;
    let umax = (1 << bits) - 1;
    let q = umax as f32 / delta;

    if value < min {
        value = min;
    }

    let mut u = ((value - min) * q) as u32;

    if u > umax {
        u = umax;
    }

    writer.write_bits(u, bits)
}

fn read_f32_range_bits(reader: &mut BitReader, min: f32, max: f32, bits: usize) -> Result<f32, io::Error> {
    let delta = max - min;
    let umax = (1 << bits) - 1;
    let q = umax as f32 / delta;

    let u = reader.read_bits(bits)?;
    let value = min + (u as f32 / q);

    Ok(value)
}

fn write_quat(writer: &mut BitWriter, quat: Quat, bits: usize) -> Result<(), io::Error> {
    let quat = quat.normalize();
    let mut largest_index = 3; // w
    let mut quat = quat.to_array();
    for i in 0..3 {
        if quat[i].abs() > quat[largest_index].abs() {
            largest_index = i;
        }
    }

    if quat[largest_index] < 0.0 {
        for i in 0..4 {
            quat[i] *= -1.0;
        }
    }

    writer.write_bits(largest_index as u32, 2)?;

    for i in 0..4 {
        if i != largest_index {
            write_f32_range_bits(writer, quat[i], -FRAC_1_SQRT_2, FRAC_1_SQRT_2, bits)?;
        }
    }

    Ok(())
}

fn read_quat(reader: &mut BitReader, bits: usize) -> Result<Quat, io::Error> {
    let largest_index = reader.read_bits(2)? as usize;

    let a = read_f32_range_bits(reader, -FRAC_1_SQRT_2, FRAC_1_SQRT_2, bits)?;
    let b = read_f32_range_bits(reader, -FRAC_1_SQRT_2, FRAC_1_SQRT_2, bits)?;
    let c = read_f32_range_bits(reader, -FRAC_1_SQRT_2, FRAC_1_SQRT_2, bits)?;

    let mut result = [0.0; 4];
    result[largest_index] = f32::sqrt(1.0 - a * a - b * b - c * c);

    let values = [a, b, c];
    let mut index_value = 0;
    for i in 0..4 {
        if i != largest_index {
            result[i] = values[index_value];
            index_value += 1;
        }
    }

    let quat = Quat::from_array(result);
    Ok(quat)
}
